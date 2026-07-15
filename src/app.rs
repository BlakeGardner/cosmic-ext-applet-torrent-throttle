// SPDX-License-Identifier: GPL-3.0

use crate::config::{ActionMode, Config};
use crate::fl;
use crate::monitor::ProcessMonitor;
use crate::qbit::{QbitClient, SpeedLimits};
use crate::tray::{QbitTray, TrayEvent};
use cosmic::app::context_drawer;
use cosmic::cosmic_config::{self, CosmicConfigEntry};
use cosmic::iced::alignment::Horizontal;
use cosmic::iced::futures::{SinkExt, StreamExt, channel::mpsc};
use cosmic::iced::{Alignment, Color, Length, Subscription, window};
use cosmic::prelude::*;
use cosmic::widget::{self, about::About, menu};
use ksni::TrayMethods;
use std::collections::HashMap;
use std::time::Duration;

const REPOSITORY: &str = env!("CARGO_PKG_REPOSITORY");

/// The application model stores app-specific state.
pub struct AppModel {
    core: cosmic::Core,
    context_page: ContextPage,
    about: About,
    key_binds: HashMap<menu::KeyBind, MenuAction>,
    config: Config,
    // App-specific state
    new_pattern: String,
    /// Whether we have actively engaged (paused or throttled).
    is_engaged: bool,
    /// Speed limits saved before throttling, to be restored later.
    saved_limits: Option<SpeedLimits>,
    matched_processes: Vec<String>,
    last_error: Option<String>,
    connection_status: Option<ConnectionStatus>,
    // Temporary text fields for throttle settings
    throttle_dl_input: String,
    throttle_ul_input: String,
    /// Handle to the system tray, once it has been spawned.
    tray: Option<ksni::Handle<QbitTray>>,
}

/// Messages emitted by the application and its widgets.
#[derive(Debug, Clone)]
pub enum Message {
    // Navigation / chrome
    LaunchUrl(String),
    ToggleContextPage(ContextPage),

    // Config updates from cosmic-config watcher
    UpdateConfig(Config),

    // Settings inputs
    SetQbitUrl(String),
    SetQbitUsername(String),
    SetQbitPassword(String),
    SetNewPattern(String),
    AddPattern(String),
    RemovePattern(usize),
    ToggleEnabled(bool),
    SetActionMode(ActionMode),
    SetThrottleDownload(String),
    SetThrottleUpload(String),
    TestConnection,

    // Monitor
    Tick,
    MonitorTick(MonitorResult),

    // Connection test result
    ConnectionResult(Result<String, String>),

    // System tray
    TrayReady(TrayHandle),
    TrayEvent(TrayEvent),
    WindowCloseRequested(window::Id),
    NoOp,
}

/// Wrapper so the tray handle can be carried inside a `Message`.
#[derive(Clone)]
pub struct TrayHandle(pub ksni::Handle<QbitTray>);

impl std::fmt::Debug for TrayHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TrayHandle")
    }
}

#[derive(Debug, Clone)]
pub struct MonitorResult {
    pub matched_processes: Vec<String>,
    pub action_taken: ActionTaken,
    pub saved_limits: Option<SpeedLimits>,
    pub error: Option<String>,
}

/// Result of the most recent connection test.
#[derive(Debug, Clone)]
pub enum ConnectionStatus {
    Testing,
    Connected(String),
    Failed(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum ActionTaken {
    Paused,
    Resumed,
    Throttled,
    Unthrottled,
    None,
}

/// The context page to display in the context drawer.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub enum ContextPage {
    #[default]
    About,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MenuAction {
    About,
}

impl menu::action::MenuAction for MenuAction {
    type Message = Message;

    fn message(&self) -> Self::Message {
        match self {
            MenuAction::About => Message::ToggleContextPage(ContextPage::About),
        }
    }
}

impl cosmic::Application for AppModel {
    type Executor = cosmic::executor::Default;
    type Flags = ();
    type Message = Message;

    const APP_ID: &'static str = "com.github.cosmic-qbit-remote";

    fn core(&self) -> &cosmic::Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut cosmic::Core {
        &mut self.core
    }

    fn init(
        core: cosmic::Core,
        _flags: Self::Flags,
    ) -> (Self, Task<cosmic::Action<Self::Message>>) {
        let about = About::default()
            .name(fl!("app-title"))
            .version(env!("CARGO_PKG_VERSION"))
            .links([(fl!("repository"), REPOSITORY)])
            .license(env!("CARGO_PKG_LICENSE"));

        let config = cosmic_config::Config::new(Self::APP_ID, Config::VERSION)
            .map(|context| match Config::get_entry(&context) {
                Ok(config) => config,
                Err((_errors, config)) => config,
            })
            .unwrap_or_default();

        let throttle_dl_input = config.throttle_download_kbps.to_string();
        let throttle_ul_input = config.throttle_upload_kbps.to_string();

        let mut app = AppModel {
            core,
            context_page: ContextPage::default(),
            about,
            key_binds: HashMap::new(),
            config,
            new_pattern: String::new(),
            is_engaged: false,
            saved_limits: None,
            matched_processes: Vec::new(),
            last_error: None,
            connection_status: None,
            throttle_dl_input,
            throttle_ul_input,
            tray: None,
        };

        let command = app.update_title();
        (app, command)
    }

    fn header_start(&self) -> Vec<Element<'_, Self::Message>> {
        let menu_bar = menu::bar(vec![menu::Tree::with_children(
            menu::root(fl!("view")).apply(Element::from),
            menu::items(
                &self.key_binds,
                vec![menu::Item::Button(fl!("about"), None, MenuAction::About)],
            ),
        )]);

        vec![menu_bar.into()]
    }

    fn context_drawer(&self) -> Option<context_drawer::ContextDrawer<'_, Self::Message>> {
        if !self.core.window.show_context {
            return None;
        }

        Some(match self.context_page {
            ContextPage::About => context_drawer::about(
                &self.about,
                |url| Message::LaunchUrl(url.to_string()),
                Message::ToggleContextPage(ContextPage::About),
            ),
        })
    }

    fn view(&self) -> Element<'_, Self::Message> {
        let spacing = cosmic::theme::spacing();

        let page = widget::container(self.view_settings(spacing.space_s))
            .max_width(640.0)
            .padding([spacing.space_s, spacing.space_l]);

        widget::scrollable(
            widget::container(page)
                .width(Length::Fill)
                .align_x(Horizontal::Center),
        )
        .height(Length::Fill)
        .into()
    }

    fn subscription(&self) -> Subscription<Self::Message> {
        let mut subscriptions = vec![
            self.core()
                .watch_config::<Config>(Self::APP_ID)
                .map(|update| Message::UpdateConfig(update.config)),
            // Intercept window close requests so the app hides to the tray.
            cosmic::iced::event::listen_with(|event, _status, id| match event {
                cosmic::iced::Event::Window(window::Event::CloseRequested) => {
                    Some(Message::WindowCloseRequested(id))
                }
                _ => None,
            }),
            // System tray service: spawned once, forwards menu events to the app.
            // Initial state is pushed by the app right after `TrayReady`.
            Subscription::run(|| {
                cosmic::iced::stream::channel(16, |mut output: mpsc::Sender<Message>| async move {
                    let (tx, mut rx) = mpsc::unbounded::<TrayEvent>();
                    let tray = QbitTray {
                        enabled: false,
                        engaged: false,
                        status_text: String::new(),
                        throttle_text: None,
                        error_text: None,
                        tx,
                    };

                    match tray.spawn().await {
                        Ok(handle) => {
                            let _ = output.send(Message::TrayReady(TrayHandle(handle))).await;
                        }
                        Err(err) => {
                            eprintln!("failed to spawn system tray: {err}");
                        }
                    }

                    while let Some(event) = rx.next().await {
                        let _ = output.send(Message::TrayEvent(event)).await;
                    }

                    std::future::pending::<()>().await;
                })
            }),
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

    fn update(&mut self, message: Self::Message) -> Task<cosmic::Action<Self::Message>> {
        let before = self.tray_snapshot();
        let task = self.handle_message(message);
        if self.tray_snapshot() == before {
            task
        } else {
            Task::batch([task, self.sync_tray()])
        }
    }
}

impl AppModel {
    fn handle_message(&mut self, message: Message) -> Task<cosmic::Action<Message>> {
        match message {
            Message::LaunchUrl(url) => {
                let _ = open::that_detached(&url);
            }

            Message::ToggleContextPage(context_page) => {
                if self.context_page == context_page {
                    self.core.window.show_context = !self.core.window.show_context;
                } else {
                    self.context_page = context_page;
                    self.core.window.show_context = true;
                }
            }

            Message::UpdateConfig(config) => {
                self.config = config;
            }

            Message::SetQbitUrl(url) => {
                self.config.qbit_url = url;
                self.save_config();
            }

            Message::SetQbitUsername(username) => {
                self.config.qbit_username = username;
                self.save_config();
            }

            Message::SetQbitPassword(password) => {
                self.config.qbit_password = password;
                self.save_config();
            }

            Message::SetNewPattern(pattern) => {
                self.new_pattern = pattern;
            }

            Message::AddPattern(submitted) => {
                let pattern = if submitted.is_empty() {
                    self.new_pattern.trim().to_string()
                } else {
                    submitted.trim().to_string()
                };
                if !pattern.is_empty() && !self.config.patterns.contains(&pattern) {
                    self.config.patterns.push(pattern);
                    self.new_pattern.clear();
                    self.save_config();
                }
            }

            Message::RemovePattern(idx) => {
                if idx < self.config.patterns.len() {
                    self.config.patterns.remove(idx);
                    self.save_config();
                }
            }

            Message::ToggleEnabled(enabled) => {
                self.config.enabled = enabled;
                self.save_config();

                if !enabled && self.is_engaged {
                    return self.disengage();
                }
            }

            Message::SetActionMode(mode) => {
                // If we're currently engaged and the mode changes, disengage first
                if self.is_engaged && self.config.action_mode != mode {
                    self.config.action_mode = mode;
                    self.save_config();
                    return self.disengage();
                }
                self.config.action_mode = mode;
                self.save_config();
            }

            Message::SetThrottleDownload(val) => {
                self.throttle_dl_input = val.clone();
                if let Ok(kbps) = val.parse::<u64>() {
                    self.config.throttle_download_kbps = kbps;
                    self.save_config();
                }
            }

            Message::SetThrottleUpload(val) => {
                self.throttle_ul_input = val.clone();
                if let Ok(kbps) = val.parse::<u64>() {
                    self.config.throttle_upload_kbps = kbps;
                    self.save_config();
                }
            }

            Message::TestConnection => {
                let url = self.config.qbit_url.clone();
                let user = self.config.qbit_username.clone();
                let pass = self.config.qbit_password.clone();
                self.connection_status = Some(ConnectionStatus::Testing);
                return cosmic::task::future(async move {
                    let client = QbitClient::new(&url, &user, &pass);
                    match client.test_connection().await {
                        Ok(version) => Message::ConnectionResult(Ok(version)),
                        Err(e) => Message::ConnectionResult(Err(e.to_string())),
                    }
                });
            }

            Message::ConnectionResult(result) => match result {
                Ok(version) => {
                    self.connection_status = Some(ConnectionStatus::Connected(version));
                }
                Err(e) => {
                    self.connection_status = Some(ConnectionStatus::Failed(e));
                }
            },

            Message::TrayReady(TrayHandle(handle)) => {
                self.tray = Some(handle);
                return self.sync_tray();
            }

            Message::TrayEvent(event) => match event {
                TrayEvent::ToggleMonitoring(enabled) => {
                    return self.handle_message(Message::ToggleEnabled(enabled));
                }
                TrayEvent::ShowWindow => return self.show_window(),
                TrayEvent::Quit => {
                    // Best-effort restore of torrents before exiting.
                    if self.is_engaged {
                        return self.disengage().chain(cosmic::iced::exit());
                    }
                    return cosmic::iced::exit();
                }
            },

            Message::WindowCloseRequested(id) => {
                if self.core.main_window_id() == Some(id) {
                    if self.tray.is_some() {
                        // Hide to tray: close the window but keep running.
                        self.core.set_main_window_id(None);
                        return window::close(id);
                    }
                    // No tray available, so closing the window quits the app.
                    return cosmic::iced::exit();
                }
                return window::close(id);
            }

            Message::NoOp => {}

            Message::MonitorTick(result) => {
                self.matched_processes = result.matched_processes;
                self.last_error = result.error;

                // Store saved limits if provided (captured before throttling)
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

            Message::Tick => {
                if !self.config.enabled || self.config.patterns.is_empty() {
                    return Task::none();
                }

                let patterns = self.config.patterns.clone();
                let url = self.config.qbit_url.clone();
                let user = self.config.qbit_username.clone();
                let pass = self.config.qbit_password.clone();
                let was_engaged = self.is_engaged;
                let action_mode = self.config.action_mode.clone();
                let throttle_dl_bytes = self.config.throttle_download_kbps * 1024;
                let throttle_ul_bytes = self.config.throttle_upload_kbps * 1024;
                let saved_limits = self.saved_limits.clone();

                return cosmic::task::future(async move {
                    let mut monitor = ProcessMonitor::new();
                    let matched = monitor.check_patterns(&patterns);
                    let has_matches = !matched.is_empty();

                    let (action, new_saved_limits, error) =
                        if has_matches && !was_engaged {
                            let client = QbitClient::new(&url, &user, &pass);
                            match action_mode {
                                ActionMode::Pause => match client.pause_all().await {
                                    Ok(()) => (ActionTaken::Paused, None, None),
                                    Err(e) => (ActionTaken::None, None, Some(e.to_string())),
                                },
                                ActionMode::Throttle => {
                                    // Save current limits before applying throttle
                                    match client.get_speed_limits().await {
                                        Ok(current) => {
                                            let target = SpeedLimits {
                                                download: throttle_dl_bytes,
                                                upload: throttle_ul_bytes,
                                            };
                                            match client.set_speed_limits(&target).await {
                                                Ok(()) => {
                                                    (ActionTaken::Throttled, Some(current), None)
                                                }
                                                Err(e) => {
                                                    (ActionTaken::None, None, Some(e.to_string()))
                                                }
                                            }
                                        }
                                        Err(e) => (ActionTaken::None, None, Some(e.to_string())),
                                    }
                                }
                            }
                        } else if !has_matches && was_engaged {
                            let client = QbitClient::new(&url, &user, &pass);
                            match action_mode {
                                ActionMode::Pause => match client.resume_all().await {
                                    Ok(()) => (ActionTaken::Resumed, None, None),
                                    Err(e) => (ActionTaken::None, None, Some(e.to_string())),
                                },
                                ActionMode::Throttle => {
                                    // Restore previously saved limits
                                    let restore = saved_limits.unwrap_or(SpeedLimits {
                                        download: 0,
                                        upload: 0,
                                    });
                                    match client.set_speed_limits(&restore).await {
                                        Ok(()) => (ActionTaken::Unthrottled, None, None),
                                        Err(e) => {
                                            (ActionTaken::None, None, Some(e.to_string()))
                                        }
                                    }
                                }
                            }
                        } else {
                            (ActionTaken::None, None, None)
                        };

                    Message::MonitorTick(MonitorResult {
                        matched_processes: matched,
                        action_taken: action,
                        saved_limits: new_saved_limits,
                        error,
                    })
                });
            }
        }

        Task::none()
    }

    fn update_title(&mut self) -> Task<cosmic::Action<Message>> {
        if let Some(id) = self.core.main_window_id() {
            self.set_window_title(fl!("app-title"), id)
        } else {
            Task::none()
        }
    }

    fn save_config(&self) {
        if let Ok(context) =
            cosmic_config::Config::new(<AppModel as cosmic::Application>::APP_ID, Config::VERSION)
        {
            if let Err(err) = self.config.write_entry(&context) {
                eprintln!("failed to save config: {err}");
            }
        }
    }

    /// Snapshot of the state mirrored to the tray, used to detect changes.
    fn tray_snapshot(&self) -> (bool, bool, String, Option<String>, Option<String>) {
        (
            self.config.enabled,
            self.is_engaged,
            self.tray_status_text(),
            self.tray_throttle_text(),
            self.tray_error_text(),
        )
    }

    fn tray_status_text(&self) -> String {
        if !self.config.enabled {
            fl!("status-disabled")
        } else if self.config.patterns.is_empty() {
            fl!("status-no-patterns")
        } else if self.is_engaged {
            let mut text = match self.config.action_mode {
                ActionMode::Pause => fl!("status-paused"),
                ActionMode::Throttle => fl!("status-throttled"),
            };
            if !self.matched_processes.is_empty() {
                text.push_str(" (");
                text.push_str(&self.matched_processes.join(", "));
                text.push(')');
            }
            text
        } else {
            fl!("status-running")
        }
    }

    fn tray_throttle_text(&self) -> Option<String> {
        (self.config.action_mode == ActionMode::Throttle).then(|| {
            fl!(
                "tray-throttle-limits",
                down = self.config.throttle_download_kbps.to_string(),
                up = self.config.throttle_upload_kbps.to_string()
            )
        })
    }

    fn tray_error_text(&self) -> Option<String> {
        self.last_error
            .as_ref()
            .map(|error| fl!("error-message", error = error.as_str()))
    }

    /// Push the current state to the tray icon and menu.
    fn sync_tray(&self) -> Task<cosmic::Action<Message>> {
        let Some(handle) = self.tray.clone() else {
            return Task::none();
        };
        let enabled = self.config.enabled;
        let engaged = self.is_engaged;
        let status_text = self.tray_status_text();
        let throttle_text = self.tray_throttle_text();
        let error_text = self.tray_error_text();

        cosmic::task::future(async move {
            handle
                .update(|tray| {
                    tray.enabled = enabled;
                    tray.engaged = engaged;
                    tray.status_text = status_text;
                    tray.throttle_text = throttle_text;
                    tray.error_text = error_text;
                })
                .await;
            Message::NoOp
        })
    }

    /// Focus the main window, reopening it if it was closed to the tray.
    fn show_window(&mut self) -> Task<cosmic::Action<Message>> {
        if let Some(id) = self.core.main_window_id() {
            return window::gain_focus(id);
        }

        let (id, task) = window::open(window::Settings {
            size: cosmic::iced::Size::new(700.0, 600.0),
            min_size: Some(cosmic::iced::Size::new(400.0, 300.0)),
            decorations: false,
            transparent: true,
            exit_on_close_request: false,
            #[cfg(target_os = "linux")]
            platform_specific: window::settings::PlatformSpecific {
                application_id: <AppModel as cosmic::Application>::APP_ID.to_string(),
                ..Default::default()
            },
            ..Default::default()
        });
        self.core.set_main_window_id(Some(id));
        Task::batch([task.map(|_| cosmic::Action::None), self.update_title()])
    }

    /// Disengage: resume torrents or restore speed limits depending on current mode.
    fn disengage(&mut self) -> Task<cosmic::Action<Message>> {
        let url = self.config.qbit_url.clone();
        let user = self.config.qbit_username.clone();
        let pass = self.config.qbit_password.clone();
        let action_mode = self.config.action_mode.clone();
        let saved_limits = self.saved_limits.clone();

        cosmic::task::future(async move {
            let client = QbitClient::new(&url, &user, &pass);
            let (action, error) = match action_mode {
                ActionMode::Pause => match client.resume_all().await {
                    Ok(()) => (ActionTaken::Resumed, None),
                    Err(e) => (ActionTaken::None, Some(e.to_string())),
                },
                ActionMode::Throttle => {
                    let restore =
                        saved_limits.unwrap_or(SpeedLimits { download: 0, upload: 0 });
                    match client.set_speed_limits(&restore).await {
                        Ok(()) => (ActionTaken::Unthrottled, None),
                        Err(e) => (ActionTaken::None, Some(e.to_string())),
                    }
                }
            };
            Message::MonitorTick(MonitorResult {
                matched_processes: Vec::new(),
                action_taken: action,
                saved_limits: None,
                error,
            })
        })
    }

    fn view_settings(&self, space_s: u16) -> Element<'_, Message> {
        let theme = cosmic::theme::active();
        let cosmic = theme.cosmic();
        let mut content = widget::column::with_capacity(16).spacing(space_s);

        content = content.push(widget::text::title3(fl!("settings-title")));

        // Monitoring toggle, mirrored with the tray menu.
        let monitoring_section = widget::settings::section().add(
            widget::settings::item::builder(fl!("monitoring"))
                .description(self.tray_status_text())
                .control(widget::toggler(self.config.enabled).on_toggle(Message::ToggleEnabled)),
        );
        content = content.push(monitoring_section);

        // qBittorrent connection settings section
        let connection_section = widget::settings::section()
            .title(fl!("connection-heading"))
            .add(
                widget::settings::item::builder(fl!("url-label")).control(
                    widget::text_input::text_input(
                        "http://localhost:8080",
                        &self.config.qbit_url,
                    )
                    .on_input(Message::SetQbitUrl),
                ),
            )
            .add(
                widget::settings::item::builder(fl!("username-label")).control(
                    widget::text_input::text_input("admin", &self.config.qbit_username)
                        .on_input(Message::SetQbitUsername),
                ),
            )
            .add(
                widget::settings::item::builder(fl!("password-label")).control(
                    widget::text_input::secure_input(
                        "password",
                        &self.config.qbit_password,
                        None::<Message>,
                        true,
                    )
                    .on_input(Message::SetQbitPassword),
                ),
            );

        content = content.push(connection_section);

        // Test connection button with colored status text
        let status_label: Option<Element<'_, Message>> = match &self.connection_status {
            Some(ConnectionStatus::Testing) => Some(widget::text::body(fl!("testing")).into()),
            Some(ConnectionStatus::Connected(version)) => Some(
                widget::text::body(fl!("connected", version = version.as_str()))
                    .class(cosmic::theme::Text::Color(Color::from(
                        cosmic.success_text_color(),
                    )))
                    .into(),
            ),
            Some(ConnectionStatus::Failed(error)) => Some(
                widget::text::body(fl!("connection-failed", error = error.as_str()))
                    .class(cosmic::theme::Text::Color(Color::from(
                        cosmic.destructive_text_color(),
                    )))
                    .into(),
            ),
            None => None,
        };

        let mut test_row = widget::row::with_capacity(2)
            .spacing(space_s)
            .align_y(Alignment::Center)
            .push(
                widget::button::standard(fl!("test-connection"))
                    .on_press(Message::TestConnection),
            );
        if let Some(label) = status_label {
            test_row = test_row.push(label);
        }
        content = content.push(test_row);

        // Action mode section
        let is_pause = self.config.action_mode == ActionMode::Pause;
        let mode_section = widget::settings::section()
            .title(fl!("action-mode-heading"))
            .add(
                widget::settings::item::builder(fl!("mode-pause-label")).control(
                    widget::radio("", true, Some(is_pause), |_| {
                        Message::SetActionMode(ActionMode::Pause)
                    }),
                ),
            )
            .add(
                widget::settings::item::builder(fl!("mode-throttle-label")).control(
                    widget::radio("", true, Some(!is_pause), |_| {
                        Message::SetActionMode(ActionMode::Throttle)
                    }),
                ),
            );

        content = content.push(mode_section);

        // Throttle settings (shown when throttle mode selected)
        if self.config.action_mode == ActionMode::Throttle {
            let throttle_section = widget::settings::section()
                .title(fl!("throttle-settings"))
                .add(
                    widget::settings::item::builder(fl!("throttle-download")).control(
                        widget::text_input::text_input("0", &self.throttle_dl_input)
                            .on_input(Message::SetThrottleDownload),
                    ),
                )
                .add(
                    widget::settings::item::builder(fl!("throttle-upload")).control(
                        widget::text_input::text_input("0", &self.throttle_ul_input)
                            .on_input(Message::SetThrottleUpload),
                    ),
                );

            content = content.push(throttle_section);
            content = content.push(widget::text::caption(fl!("throttle-hint")));
        }

        // Pattern management section
        content = content.push(widget::text::heading(fl!("patterns-heading")));
        content = content.push(widget::text::caption(fl!("patterns-description")));

        // Add pattern input
        content = content.push(
            widget::row::with_capacity(2)
                .spacing(space_s)
                .align_y(Alignment::Center)
                .push(
                    widget::text_input::text_input(
                        fl!("pattern-placeholder"),
                        &self.new_pattern,
                    )
                    .on_input(Message::SetNewPattern)
                    .on_submit(Message::AddPattern),
                )
                .push(
                    widget::button::standard(fl!("add"))
                        .on_press(Message::AddPattern(String::new())),
                ),
        );

        // Pattern list
        if !self.config.patterns.is_empty() {
            let mut patterns_section = widget::settings::section();
            for (idx, pattern) in self.config.patterns.iter().enumerate() {
                patterns_section = patterns_section.add(
                    widget::settings::item::builder(pattern.clone()).control(
                        widget::button::destructive(fl!("remove"))
                            .on_press(Message::RemovePattern(idx)),
                    ),
                );
            }
            content = content.push(patterns_section);
        }

        content.width(Length::Fill).into()
    }
}
