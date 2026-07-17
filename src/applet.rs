// SPDX-License-Identifier: GPL-3.0

//! Native COSMIC panel applet: a panel icon with a popup containing a real
//! toggle switch, matching the Wi-Fi and Bluetooth applets.

use crate::config::{ActionMode, Config, MonitorState, QuitSignal};
use crate::engine::{self, ActionTaken, MonitorResult};
use crate::fl;
use crate::qbit::SpeedLimits;
use cosmic::app;
use cosmic::applet::{menu_button, padded_control};
use cosmic::cosmic_config::{self, ConfigGet, ConfigSet, CosmicConfigEntry};
use cosmic::cosmic_theme::Spacing;
use cosmic::iced::platform_specific::shell::wayland::commands::popup::destroy_popup;
use cosmic::iced::{Length, Subscription, window};
use cosmic::widget::{divider, text, toggler};
use cosmic::{Element, Task, theme};
use cosmic::iced::futures::{SinkExt, channel::mpsc};
use std::time::Duration;

/// Config ID shared with the settings application.
const CONFIG_ID: &str = "io.github.BlakeGardner.cosmic-ext-applet-qbit-remote";

/// Desktop entry ID the panel uses to spawn the applet.
const APPLET_ID: &str = "io.github.BlakeGardner.cosmic-ext-applet-qbit-remote.Applet";

pub fn run() -> cosmic::iced::Result {
    cosmic::applet::run::<QbitApplet>(())
}

/// Make sure the panel applet is running.
///
/// cosmic-panel spawns the applets listed in its config at startup but never
/// respawns one that exited (e.g. via the applet's Quit action). When the
/// settings app starts, add the applet to the panel on first run, or briefly
/// remove and re-add its config entry so the panel spawns it again.
pub fn ensure_applet_running() {
    if applet_process_running() {
        return;
    }

    let panels = cosmic_config::Config::new("com.system76.CosmicPanel", 1)
        .ok()
        .and_then(|config| config.get::<Vec<String>>("entries").ok())
        .unwrap_or_else(|| vec![String::from("Panel")]);

    let mut listed = false;
    for panel in &panels {
        let Ok(config) = cosmic_config::Config::new(&format!("com.system76.CosmicPanel.{panel}"), 1)
        else {
            continue;
        };

        let wings = config
            .get::<Option<(Vec<String>, Vec<String>)>>("plugins_wings")
            .ok()
            .flatten();
        if let Some(wings) = wings.filter(|(left, right)| {
            left.iter().chain(right).any(|id| id == APPLET_ID)
        }) {
            listed = true;
            let without = wings_without_applet(&wings);
            respawn_via_toggle(&config, "plugins_wings", &Some(wings), &Some(without));
            continue;
        }

        let center = config
            .get::<Option<Vec<String>>>("plugins_center")
            .ok()
            .flatten();
        if let Some(center) = center.filter(|ids| ids.iter().any(|id| id == APPLET_ID)) {
            listed = true;
            let without: Vec<String> =
                center.iter().filter(|id| *id != APPLET_ID).cloned().collect();
            respawn_via_toggle(&config, "plugins_center", &Some(center), &Some(without));
        }
    }

    // First run: add the applet to the right wing of the first panel.
    if !listed {
        if let Some(panel) = panels.first() {
            if let Ok(config) =
                cosmic_config::Config::new(&format!("com.system76.CosmicPanel.{panel}"), 1)
            {
                let (left, mut right) = config
                    .get::<Option<(Vec<String>, Vec<String>)>>("plugins_wings")
                    .ok()
                    .flatten()
                    .unwrap_or_default();
                right.insert(0, String::from(APPLET_ID));
                let _ = config.set("plugins_wings", Some((left, right)));
            }
        }
    }
}

fn wings_without_applet(wings: &(Vec<String>, Vec<String>)) -> (Vec<String>, Vec<String>) {
    let strip = |ids: &[String]| ids.iter().filter(|id| *id != APPLET_ID).cloned().collect();
    (strip(&wings.0), strip(&wings.1))
}

/// The panel only reacts to config changes, so write the plugin list without
/// the applet, give the panel a moment to notice, then restore the original.
fn respawn_via_toggle<T: serde::Serialize>(
    config: &cosmic_config::Config,
    key: &str,
    original: &T,
    without: &T,
) {
    if config.set(key, without).is_ok() {
        std::thread::sleep(Duration::from_millis(1500));
    }
    let _ = config.set(key, original);
}

/// Whether any applet instance of this application is currently running,
/// detected by probing the leader lock (works from inside a Flatpak
/// sandbox, where other instances' processes are invisible).
fn applet_process_running() -> bool {
    // If the lock can be acquired no leader is running. A running follower
    // without a leader can only happen after a leader crash, and the
    // respawn toggle below replaces those instances anyway.
    try_acquire_leadership().is_none()
}

/// The panel spawns one applet process per panel/output. Only the process
/// holding this lock runs the monitoring engine; the others mirror its
/// state via cosmic-config so every popup shows the same status.
fn try_acquire_leadership() -> Option<std::fs::File> {
    let dir = crate::sandbox::shared_runtime_dir();
    let _ = std::fs::create_dir_all(&dir);
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join("cosmic-ext-applet-qbit-remote-applet.lock"))
        .ok()?;
    file.try_lock().ok().map(|()| file)
}

/// Broadcast a quit to every applet instance via cosmic-config state.
/// Unlike signals, this crosses Flatpak sandbox boundaries.
fn broadcast_quit() {
    let signal = QuitSignal {
        quit_at_millis: now_millis(),
    };
    if let Ok(context) = cosmic_config::Config::new_state(CONFIG_ID, QuitSignal::VERSION) {
        let _ = signal.write_entry(&context);
    }
}

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or_default()
}

struct QbitApplet {
    core: cosmic::Core,
    popup: Option<window::Id>,
    config: Config,
    /// Held for the process lifetime; `Some` means this instance runs the engine.
    leader_lock: Option<std::fs::File>,
    /// Used to ignore quit broadcasts that predate this instance.
    started_at_millis: u64,
    is_engaged: bool,
    saved_limits: Option<SpeedLimits>,
    matched_processes: Vec<String>,
    last_error: Option<String>,
}

#[derive(Debug, Clone)]
enum Message {
    TogglePopup,
    CloseRequested(window::Id),
    ToggleMonitoring(bool),
    UpdateConfig(Config),
    UpdateState(MonitorState),
    QuitRequested(QuitSignal),
    Tick,
    MonitorTick(MonitorResult),
    OpenSettings,
    Quit,
}

impl QbitApplet {
    fn is_leader(&self) -> bool {
        self.leader_lock.is_some()
    }

    fn icon_name(&self) -> &'static str {
        if !self.config.enabled {
            "network-disconnected-symbolic"
        } else if self.is_engaged {
            "media-playback-pause-symbolic"
        } else {
            "network-transmit-receive-symbolic"
        }
    }

    fn status_text(&self) -> String {
        if !self.config.enabled {
            fl!("status-disabled")
        } else if self.config.patterns.is_empty() {
            fl!("status-no-patterns")
        } else if self.is_engaged {
            match self.config.action_mode {
                ActionMode::Pause => fl!("status-paused"),
                ActionMode::Throttle => fl!("status-throttled"),
            }
        } else {
            fl!("status-running")
        }
    }

    fn save_config(&self) {
        if let Ok(context) = cosmic_config::Config::new(CONFIG_ID, Config::VERSION) {
            if let Err(err) = self.config.write_entry(&context) {
                eprintln!("failed to save config: {err}");
            }
        }
    }

    /// Publish the leader's monitoring state so other instances mirror it.
    fn save_state(&self) {
        let state = MonitorState {
            is_engaged: self.is_engaged,
            matched_processes: self.matched_processes.clone(),
            last_error: self.last_error.clone().unwrap_or_default(),
        };
        if let Ok(context) = cosmic_config::Config::new_state(CONFIG_ID, MonitorState::VERSION) {
            if let Err(err) = state.write_entry(&context) {
                eprintln!("failed to save monitor state: {err}");
            }
        }
    }

    fn disengage(&mut self) -> Task<cosmic::Action<Message>> {
        let config = self.config.clone();
        let saved_limits = self.saved_limits.take();
        cosmic::task::future(async move {
            let (action, error) = engine::disengage(&config, saved_limits).await;
            Message::MonitorTick(MonitorResult {
                matched_processes: Vec::new(),
                action_taken: action,
                saved_limits: None,
                error,
            })
        })
    }
}

impl cosmic::Application for QbitApplet {
    type Executor = cosmic::executor::Default;
    type Flags = ();
    type Message = Message;

    const APP_ID: &'static str = "io.github.BlakeGardner.cosmic-ext-applet-qbit-remote.Applet";

    fn core(&self) -> &cosmic::Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut cosmic::Core {
        &mut self.core
    }

    fn init(core: cosmic::Core, _flags: Self::Flags) -> (Self, app::Task<Self::Message>) {
        let config = cosmic_config::Config::new(CONFIG_ID, Config::VERSION)
            .map(|context| match Config::get_entry(&context) {
                Ok(config) => config,
                Err((_errors, config)) => config,
            })
            .unwrap_or_default();

        let leader_lock = try_acquire_leadership();

        // Followers adopt the leader's published state.
        let state = if leader_lock.is_none() {
            cosmic_config::Config::new_state(CONFIG_ID, MonitorState::VERSION)
                .ok()
                .and_then(|context| MonitorState::get_entry(&context).ok())
                .unwrap_or_default()
        } else {
            MonitorState::default()
        };

        let applet = Self {
            core,
            popup: None,
            config,
            leader_lock,
            started_at_millis: now_millis(),
            is_engaged: state.is_engaged,
            saved_limits: None,
            matched_processes: state.matched_processes,
            last_error: (!state.last_error.is_empty()).then_some(state.last_error),
        };

        // Reset any stale state left over from a previous leader.
        if applet.is_leader() {
            applet.save_state();
        }

        (applet, Task::none())
    }

    fn update(&mut self, message: Self::Message) -> app::Task<Self::Message> {
        match message {
            Message::TogglePopup => {
                if let Some(popup) = self.popup.take() {
                    return destroy_popup(popup);
                }
                return cosmic::surface::surface_task(cosmic::surface::action::app_popup(
                    |_: &Self| Default::default(),
                    move |app: &mut Self| {
                        let new_id = window::Id::unique();
                        app.popup.replace(new_id);
                        app.core.applet.get_popup_settings(
                            app.core.main_window_id().unwrap(),
                            new_id,
                            None,
                            None,
                            None,
                        )
                    },
                    None,
                ));
            }

            Message::CloseRequested(id) => {
                if self.popup == Some(id) {
                    self.popup = None;
                }
            }

            Message::ToggleMonitoring(enabled) => {
                self.config.enabled = enabled;
                self.save_config();

                if self.is_leader() && !enabled && self.is_engaged {
                    return self.disengage();
                }
            }

            Message::UpdateConfig(config) => {
                if self.is_leader() {
                    // If the mode changed while engaged, disengage with the old
                    // mode's semantics before adopting the new config.
                    if self.is_engaged && self.config.action_mode != config.action_mode {
                        let task = self.disengage();
                        self.config = config;
                        return task;
                    }
                    let disable = self.is_engaged && !config.enabled;
                    self.config = config;
                    if disable {
                        return self.disengage();
                    }
                } else {
                    self.config = config;
                }
            }

            Message::UpdateState(state) => {
                // Mirror the leader's monitoring state.
                if !self.is_leader() {
                    self.is_engaged = state.is_engaged;
                    self.matched_processes = state.matched_processes;
                    self.last_error = (!state.last_error.is_empty()).then_some(state.last_error);
                }
            }

            Message::Tick => {
                if !self.config.enabled || self.config.patterns.is_empty() {
                    return Task::none();
                }
                let config = self.config.clone();
                let was_engaged = self.is_engaged;
                let saved_limits = self.saved_limits.clone();
                return cosmic::task::future(async move {
                    Message::MonitorTick(engine::run_cycle(config, was_engaged, saved_limits).await)
                });
            }

            Message::MonitorTick(result) => {
                self.matched_processes = result.matched_processes;
                self.last_error = result.error;

                if let Some(limits) = result.saved_limits {
                    self.saved_limits = Some(limits);
                }

                match result.action_taken {
                    ActionTaken::Paused | ActionTaken::Throttled => self.is_engaged = true,
                    ActionTaken::Resumed | ActionTaken::Unthrottled => {
                        self.is_engaged = false;
                        self.saved_limits = None;
                    }
                    ActionTaken::None => {}
                }

                self.save_state();
            }

            Message::OpenSettings => {
                // Launch the settings window (this same binary without --applet).
                if let Ok(exe) = std::env::current_exe() {
                    let _ = std::process::Command::new(exe).spawn();
                }
                if let Some(popup) = self.popup.take() {
                    return destroy_popup(popup);
                }
            }

            Message::QuitRequested(signal) => {
                // Another instance quit; follow suit, but ignore stale
                // broadcasts left over from a previous session.
                if signal.quit_at_millis > self.started_at_millis {
                    if self.is_leader() && self.is_engaged {
                        return self.disengage().chain(cosmic::iced::exit());
                    }
                    return cosmic::iced::exit();
                }
            }

            Message::Quit => {
                // Quit every panel's instance, not just this one. The
                // broadcast reaches siblings even across Flatpak sandboxes.
                broadcast_quit();

                // Best-effort restore of torrents before exiting.
                if self.is_leader() && self.is_engaged {
                    return self.disengage().chain(cosmic::iced::exit());
                }
                return cosmic::iced::exit();
            }
        }

        Task::none()
    }

    fn view(&self) -> Element<'_, Self::Message> {
        self.core
            .applet
            .icon_button(self.icon_name())
            .on_press_down(Message::TogglePopup)
            .into()
    }

    fn view_window(&self, _id: window::Id) -> Element<'_, Self::Message> {
        let Spacing {
            space_xxs, space_s, ..
        } = theme::spacing();

        let mut content = cosmic::iced::widget::column![padded_control(
            toggler(self.config.enabled)
                .label(fl!("monitoring"))
                .on_toggle(Message::ToggleMonitoring)
                .width(Length::Fill)
                .text_size(14)
        )]
        .padding([8, 0]);

        content = content
            .push(padded_control(divider::horizontal::default()).padding([space_xxs, space_s]))
            .push(padded_control(text::body(self.status_text())));

        if !self.matched_processes.is_empty() {
            content = content.push(padded_control(text::caption(
                self.matched_processes.join(", "),
            )));
        }

        if self.config.action_mode == ActionMode::Throttle {
            content = content.push(padded_control(text::caption(fl!(
                "throttle-status",
                down = self.config.throttle_download_kbps.to_string(),
                up = self.config.throttle_upload_kbps.to_string()
            ))));
        }

        if let Some(ref error) = self.last_error {
            content = content.push(padded_control(text::caption(fl!(
                "error-message",
                error = error.as_str()
            ))));
        }

        content = content
            .push(padded_control(divider::horizontal::default()).padding([space_xxs, space_s]))
            .push(
                menu_button(text::body(fl!("settings-title"))).on_press(Message::OpenSettings),
            )
            .push(menu_button(text::body(fl!("quit"))).on_press(Message::Quit));

        self.core.applet.popup_container(content).into()
    }

    fn subscription(&self) -> Subscription<Self::Message> {
        let mut subscriptions = vec![
            self.core()
                .watch_config::<Config>(CONFIG_ID)
                .map(|update| Message::UpdateConfig(update.config)),
            // Quit gracefully on SIGTERM (e.g. panel shutdown).
            Subscription::run(|| {
                cosmic::iced::stream::channel(1, |mut output: mpsc::Sender<Message>| async move {
                    if let Ok(mut term) =
                        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                    {
                        term.recv().await;
                        let _ = output.send(Message::Quit).await;
                    }
                    std::future::pending::<()>().await
                })
            }),
            // Quit when any other instance broadcasts a quit.
            self.core()
                .watch_state::<QuitSignal>(CONFIG_ID)
                .map(|update| Message::QuitRequested(update.config)),
        ];

        if self.is_leader() {
            if self.config.enabled && !self.config.patterns.is_empty() {
                let interval_secs = self.config.poll_interval_secs.max(5);
                subscriptions.push(
                    cosmic::iced::time::every(Duration::from_secs(interval_secs))
                        .map(|_| Message::Tick),
                );
            }
        } else {
            subscriptions.push(
                self.core()
                    .watch_state::<MonitorState>(CONFIG_ID)
                    .map(|update| Message::UpdateState(update.config)),
            );
        }

        Subscription::batch(subscriptions)
    }

    fn style(&self) -> Option<cosmic::iced::theme::Style> {
        Some(cosmic::applet::style())
    }

    fn on_close_requested(&self, id: window::Id) -> Option<Message> {
        Some(Message::CloseRequested(id))
    }
}
