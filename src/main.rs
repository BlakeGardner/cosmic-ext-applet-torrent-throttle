mod config;
mod monitor;
mod qbit;

use config::AppConfig;
use cosmic::app::{Core, Settings, Task};
use cosmic::iced::{self, Alignment, Length, Subscription};
use cosmic::prelude::*;
use cosmic::widget;
use monitor::ProcessMonitor;
use qbit::QbitClient;
use std::time::Duration;

const APP_ID: &str = "com.github.cosmic-qbit-remote";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let settings = Settings::default().size(iced::Size::new(550.0, 600.0));
    cosmic::app::run::<App>(settings, ())?;
    Ok(())
}

#[derive(Clone, Debug)]
enum Message {
    // Settings
    SetQbitUrl(String),
    SetQbitUsername(String),
    SetQbitPassword(String),
    SetNewPattern(String),
    AddPattern(String),
    RemovePattern(usize),
    ToggleEnabled(bool),
    TestConnection,
    SaveSettings,

    // Monitor
    Tick,
    TickResult(MonitorResult),

    // Navigation
    ShowSettings,
    ShowStatus,

    // Connection test result
    ConnectionResult(Result<String, String>),
}

#[derive(Debug, Clone)]
struct MonitorResult {
    matched_processes: Vec<String>,
    action_taken: ActionTaken,
    error: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
enum ActionTaken {
    Paused,
    Resumed,
    None,
}

#[derive(Debug, Clone, PartialEq)]
enum Page {
    Status,
    Settings,
}

struct App {
    core: Core,
    config: AppConfig,
    page: Page,
    new_pattern: String,
    enabled: bool,
    torrents_paused: bool,
    matched_processes: Vec<String>,
    last_error: Option<String>,
    connection_status: Option<String>,
}

impl cosmic::Application for App {
    type Executor = cosmic::executor::Default;
    type Flags = ();
    type Message = Message;

    const APP_ID: &'static str = APP_ID;

    fn core(&self) -> &Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    fn init(mut core: Core, _flags: Self::Flags) -> (Self, Task<Self::Message>) {
        core.set_header_title("qBit Process Monitor".to_string());

        let config = AppConfig::load();
        let enabled = config.enabled;

        let app = Self {
            core,
            config,
            page: Page::Status,
            new_pattern: String::new(),
            enabled,
            torrents_paused: false,
            matched_processes: Vec::new(),
            last_error: None,
            connection_status: None,
        };

        (app, Task::none())
    }

    fn subscription(&self) -> Subscription<Self::Message> {
        if self.enabled {
            let secs = self.config.poll_interval_secs;
            iced::time::every(Duration::from_secs(secs)).map(|_| Message::Tick)
        } else {
            Subscription::none()
        }
    }

    fn update(&mut self, message: Self::Message) -> Task<Self::Message> {
        match message {
            Message::SetQbitUrl(url) => {
                self.config.qbit_url = url;
            }
            Message::SetQbitUsername(username) => {
                self.config.qbit_username = username;
            }
            Message::SetQbitPassword(password) => {
                self.config.qbit_password = password;
            }
            Message::SetNewPattern(pattern) => {
                self.new_pattern = pattern;
            }
            Message::AddPattern(submitted) => {
                // Use submitted value from on_submit, or fall back to current input
                let pattern = if submitted.is_empty() {
                    self.new_pattern.trim().to_string()
                } else {
                    submitted.trim().to_string()
                };
                if !pattern.is_empty() && !self.config.patterns.contains(&pattern) {
                    self.config.patterns.push(pattern);
                    self.new_pattern.clear();
                    self.config.save();
                }
            }
            Message::RemovePattern(idx) => {
                if idx < self.config.patterns.len() {
                    self.config.patterns.remove(idx);
                    self.config.save();
                }
            }
            Message::ToggleEnabled(enabled) => {
                self.enabled = enabled;
                self.config.enabled = enabled;
                self.config.save();

                if !enabled && self.torrents_paused {
                    let url = self.config.qbit_url.clone();
                    let user = self.config.qbit_username.clone();
                    let pass = self.config.qbit_password.clone();
                    return cosmic::task::future(async move {
                        let client = QbitClient::new(&url, &user, &pass);
                        let _ = client.resume_all().await;
                        Message::TickResult(MonitorResult {
                            matched_processes: Vec::new(),
                            action_taken: ActionTaken::Resumed,
                            error: None,
                        })
                    });
                }
            }
            Message::TestConnection => {
                let url = self.config.qbit_url.clone();
                let user = self.config.qbit_username.clone();
                let pass = self.config.qbit_password.clone();
                self.connection_status = Some("Testing...".to_string());
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
                    self.connection_status =
                        Some(format!("✓ Connected! qBittorrent {}", version));
                }
                Err(e) => {
                    self.connection_status = Some(format!("✗ Failed: {}", e));
                }
            },
            Message::SaveSettings => {
                self.config.save();
            }
            Message::ShowSettings => {
                self.page = Page::Settings;
            }
            Message::ShowStatus => {
                self.page = Page::Status;
            }
            Message::Tick => {
                if !self.enabled || self.config.patterns.is_empty() {
                    return Task::none();
                }

                let patterns = self.config.patterns.clone();
                let url = self.config.qbit_url.clone();
                let user = self.config.qbit_username.clone();
                let pass = self.config.qbit_password.clone();
                let was_paused = self.torrents_paused;

                return cosmic::task::future(async move {
                    let mut monitor = ProcessMonitor::new();
                    let matched = monitor.check_patterns(&patterns);
                    let has_matches = !matched.is_empty();

                    let (action, error) = if has_matches && !was_paused {
                        let client = QbitClient::new(&url, &user, &pass);
                        match client.pause_all().await {
                            Ok(()) => (ActionTaken::Paused, None),
                            Err(e) => (ActionTaken::None, Some(e.to_string())),
                        }
                    } else if !has_matches && was_paused {
                        let client = QbitClient::new(&url, &user, &pass);
                        match client.resume_all().await {
                            Ok(()) => (ActionTaken::Resumed, None),
                            Err(e) => (ActionTaken::None, Some(e.to_string())),
                        }
                    } else {
                        (ActionTaken::None, None)
                    };

                    Message::TickResult(MonitorResult {
                        matched_processes: matched,
                        action_taken: action,
                        error,
                    })
                });
            }
            Message::TickResult(result) => {
                self.matched_processes = result.matched_processes;
                self.last_error = result.error;

                match result.action_taken {
                    ActionTaken::Paused => self.torrents_paused = true,
                    ActionTaken::Resumed => self.torrents_paused = false,
                    ActionTaken::None => {}
                }
            }
        }

        Task::none()
    }

    fn view(&self) -> Element<'_, Self::Message> {
        match self.page {
            Page::Status => self.view_status(),
            Page::Settings => self.view_settings(),
        }
    }
}

impl App {
    fn view_status(&self) -> Element<'_, Message> {
        let mut content = widget::column::with_capacity(10).spacing(16).padding(24);

        content = content.push(widget::text::title3("qBittorrent Process Monitor"));

        // Enable toggle
        content = content.push(
            widget::Row::new()
                .spacing(12)
                .align_y(Alignment::Center)
                .push(widget::text::body("Monitoring"))
                .push(widget::toggler(self.enabled).on_toggle(Message::ToggleEnabled)),
        );

        // Status indicator
        let status_text = if !self.enabled {
            "⏸ Monitoring disabled"
        } else if self.config.patterns.is_empty() {
            "⚠ No patterns configured"
        } else if self.torrents_paused {
            "⏸ Torrents PAUSED — matched processes detected"
        } else {
            "▶ Torrents running — no matches"
        };
        content = content.push(widget::text::body(status_text));

        // Matched processes
        if !self.matched_processes.is_empty() {
            content = content.push(widget::text::heading("Matched Processes:"));
            for proc in &self.matched_processes {
                content = content.push(
                    widget::Row::new()
                        .spacing(8)
                        .push(widget::text::body("•"))
                        .push(widget::text::body(proc.as_str())),
                );
            }
        }

        // Watched patterns
        if !self.config.patterns.is_empty() {
            content = content.push(widget::text::heading("Watching for:"));
            for pattern in &self.config.patterns {
                content = content.push(
                    widget::Row::new()
                        .spacing(8)
                        .push(widget::text::body("•"))
                        .push(widget::text::body(pattern.as_str())),
                );
            }
        }

        // Error display
        if let Some(ref error) = self.last_error {
            content = content.push(widget::text::body(format!("Error: {}", error)));
        }

        // Settings button
        content = content.push(
            widget::button::standard("Settings").on_press(Message::ShowSettings),
        );

        widget::container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn view_settings(&self) -> Element<'_, Message> {
        let mut content = widget::column::with_capacity(16).spacing(16).padding(24);

        content = content.push(widget::text::title3("Settings"));

        // qBittorrent connection settings
        content = content.push(widget::text::heading("qBittorrent Connection"));

        content = content.push(
            widget::column::with_capacity(2)
                .spacing(4)
                .push(widget::text::body("URL"))
                .push(
                    widget::text_input::text_input(
                        "http://localhost:8080",
                        &self.config.qbit_url,
                    )
                    .on_input(Message::SetQbitUrl),
                ),
        );

        content = content.push(
            widget::column::with_capacity(2)
                .spacing(4)
                .push(widget::text::body("Username"))
                .push(
                    widget::text_input::text_input("admin", &self.config.qbit_username)
                        .on_input(Message::SetQbitUsername),
                ),
        );

        content = content.push(
            widget::column::with_capacity(2)
                .spacing(4)
                .push(widget::text::body("Password"))
                .push(
                    widget::text_input::secure_input(
                        "password",
                        &self.config.qbit_password,
                        None::<Message>,
                        true,
                    )
                    .on_input(Message::SetQbitPassword),
                ),
        );

        // Test connection button
        content = content.push(
            widget::Row::new()
                .spacing(12)
                .align_y(Alignment::Center)
                .push(
                    widget::button::standard("Test Connection")
                        .on_press(Message::TestConnection),
                )
                .push(widget::text::body(
                    self.connection_status.as_deref().unwrap_or(""),
                )),
        );

        // Pattern management
        content = content.push(widget::text::heading("Process Patterns"));
        content = content.push(widget::text::caption(
            "Torrents will be paused when any process matching these patterns is running.",
        ));

        // Add pattern input
        content = content.push(
            widget::Row::new()
                .spacing(8)
                .align_y(Alignment::Center)
                .push(
                    widget::text_input::text_input("e.g. steam, firefox", &self.new_pattern)
                        .on_input(Message::SetNewPattern)
                        .on_submit(Message::AddPattern),
                )
                .push(
                    widget::button::standard("Add")
                        .on_press(Message::AddPattern(String::new())),
                ),
        );

        // Pattern list
        for (idx, pattern) in self.config.patterns.iter().enumerate() {
            content = content.push(
                widget::Row::new()
                    .spacing(8)
                    .align_y(Alignment::Center)
                    .push(
                        widget::text::body(pattern.as_str()).width(Length::Fill),
                    )
                    .push(
                        widget::button::destructive("Remove")
                            .on_press(Message::RemovePattern(idx)),
                    ),
            );
        }

        // Save / Back buttons
        content = content.push(
            widget::Row::new()
                .spacing(12)
                .push(widget::button::suggested("Save").on_press(Message::SaveSettings))
                .push(widget::button::standard("Back").on_press(Message::ShowStatus)),
        );

        widget::container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}
