// SPDX-License-Identifier: GPL-3.0

use crate::config::{ActionMode, Config};
use crate::fl;
use crate::qbit::QbitClient;
use cosmic::app::context_drawer;
use cosmic::cosmic_config::{self, CosmicConfigEntry};
use cosmic::iced::alignment::Horizontal;
use cosmic::iced::{Alignment, Color, Length, Subscription};
use cosmic::prelude::*;
use cosmic::widget::{self, about::About, menu};
use std::collections::HashMap;

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
    connection_status: Option<ConnectionStatus>,
    // Temporary text fields for throttle settings
    throttle_dl_input: String,
    throttle_ul_input: String,
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

    // Connection test result
    ConnectionResult(Result<String, String>),
}

/// Result of the most recent connection test.
#[derive(Debug, Clone)]
pub enum ConnectionStatus {
    Testing,
    Connected(String),
    Failed(String),
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

    const APP_ID: &'static str = "io.github.BlakeGardner.cosmic-ext-applet-torrent-throttle";

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
            connection_status: None,
            throttle_dl_input,
            throttle_ul_input,
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
        self.core()
            .watch_config::<Config>(Self::APP_ID)
            .map(|update| Message::UpdateConfig(update.config))
    }

    fn update(&mut self, message: Self::Message) -> Task<cosmic::Action<Self::Message>> {
        self.handle_message(message)
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
                // The applet watches the config and engages/disengages itself.
                self.config.enabled = enabled;
                self.save_config();
            }

            Message::SetActionMode(mode) => {
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

    /// Config-derived monitoring status shown under the toggle in Settings.
    fn monitoring_description(&self) -> String {
        if !self.config.enabled {
            fl!("status-disabled")
        } else if self.config.patterns.is_empty() {
            fl!("status-no-patterns")
        } else {
            fl!("monitoring-description")
        }
    }

    fn view_settings(&self, space_s: u16) -> Element<'_, Message> {
        let theme = cosmic::theme::active();
        let cosmic = theme.cosmic();
        let mut content = widget::column::with_capacity(16).spacing(space_s);

        content = content.push(widget::text::title3(fl!("settings-title")));

        // Monitoring toggle, mirrored with the panel applet.
        let monitoring_section = widget::settings::section().add(
            widget::settings::item::builder(fl!("monitoring"))
                .description(self.monitoring_description())
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
