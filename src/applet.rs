// SPDX-License-Identifier: GPL-3.0

//! Native COSMIC panel applet: a panel icon with a popup containing a real
//! toggle switch, matching the Wi-Fi and Bluetooth applets.

use crate::config::{ActionMode, Config};
use crate::engine::{self, ActionTaken, MonitorResult};
use crate::fl;
use crate::qbit::SpeedLimits;
use cosmic::app;
use cosmic::applet::{menu_button, padded_control};
use cosmic::cosmic_config::{self, CosmicConfigEntry};
use cosmic::cosmic_theme::Spacing;
use cosmic::iced::platform_specific::shell::wayland::commands::popup::destroy_popup;
use cosmic::iced::{Length, Subscription, window};
use cosmic::widget::{divider, text, toggler};
use cosmic::{Element, Task, theme};
use std::time::Duration;

/// Config ID shared with the settings application.
const CONFIG_ID: &str = "com.github.cosmic-qbit-remote";

pub fn run() -> cosmic::iced::Result {
    cosmic::applet::run::<QbitApplet>(())
}

struct QbitApplet {
    core: cosmic::Core,
    popup: Option<window::Id>,
    config: Config,
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
    Tick,
    MonitorTick(MonitorResult),
    OpenSettings,
}

impl QbitApplet {
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

    const APP_ID: &'static str = "com.github.cosmic-qbit-remote.Applet";

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

        (
            Self {
                core,
                popup: None,
                config,
                is_engaged: false,
                saved_limits: None,
                matched_processes: Vec::new(),
                last_error: None,
            },
            Task::none(),
        )
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

                if !enabled && self.is_engaged {
                    return self.disengage();
                }
            }

            Message::UpdateConfig(config) => {
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
            );

        self.core.applet.popup_container(content).into()
    }

    fn subscription(&self) -> Subscription<Self::Message> {
        let mut subscriptions = vec![
            self.core()
                .watch_config::<Config>(CONFIG_ID)
                .map(|update| Message::UpdateConfig(update.config)),
        ];

        if self.config.enabled && !self.config.patterns.is_empty() {
            let interval_secs = self.config.poll_interval_secs.max(5);
            subscriptions.push(
                cosmic::iced::time::every(Duration::from_secs(interval_secs))
                    .map(|_| Message::Tick),
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
