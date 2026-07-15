// SPDX-License-Identifier: GPL-3.0

use crate::fl;
use cosmic::iced::futures::channel::mpsc::UnboundedSender;

/// Events sent from the tray menu to the application.
#[derive(Debug, Clone)]
pub enum TrayEvent {
    ToggleMonitoring(bool),
    ShowWindow,
    Quit,
}

/// State mirrored from the application into the tray icon and menu.
pub struct QbitTray {
    pub enabled: bool,
    pub engaged: bool,
    pub status_text: String,
    pub throttle_text: Option<String>,
    pub error_text: Option<String>,
    pub tx: UnboundedSender<TrayEvent>,
}

impl QbitTray {
    fn send(&self, event: TrayEvent) {
        let _ = self.tx.unbounded_send(event);
    }
}

impl ksni::Tray for QbitTray {
    // Open the menu on left-click (sets ItemIsMenu), matching the behavior of
    // the COSMIC Wi-Fi and Bluetooth tray icons.
    const MENU_ON_ACTIVATE: bool = true;

    fn id(&self) -> String {
        env!("CARGO_PKG_NAME").into()
    }

    fn title(&self) -> String {
        fl!("app-title")
    }

    fn icon_name(&self) -> String {
        if !self.enabled {
            // Monitoring off: disconnected network glyph.
            "network-disconnected-symbolic".into()
        } else if self.engaged {
            // Throttle engaged: pause glyph.
            "media-playback-pause-symbolic".into()
        } else {
            // Monitoring, torrents at full speed: up/down transfer arrows.
            "network-transmit-receive-symbolic".into()
        }
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        let mut description = self.status_text.clone();
        if let Some(ref throttle) = self.throttle_text {
            description.push('\n');
            description.push_str(throttle);
        }
        if let Some(ref error) = self.error_text {
            description.push('\n');
            description.push_str(error);
        }
        ksni::ToolTip {
            title: fl!("app-title"),
            description,
            ..Default::default()
        }
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::{CheckmarkItem, StandardItem};

        // Primary control first, like the COSMIC Wi-Fi/Bluetooth menus.
        let mut items: Vec<ksni::MenuItem<Self>> = vec![
            CheckmarkItem {
                label: fl!("tray-monitoring"),
                checked: self.enabled,
                activate: Box::new(|tray: &mut Self| {
                    tray.enabled = !tray.enabled;
                    tray.send(TrayEvent::ToggleMonitoring(tray.enabled));
                }),
                ..Default::default()
            }
            .into(),
            ksni::MenuItem::Separator,
            StandardItem {
                label: self.status_text.clone(),
                enabled: false,
                ..Default::default()
            }
            .into(),
        ];

        if let Some(ref throttle) = self.throttle_text {
            items.push(
                StandardItem {
                    label: throttle.clone(),
                    enabled: false,
                    ..Default::default()
                }
                .into(),
            );
        }

        if let Some(ref error) = self.error_text {
            items.push(
                StandardItem {
                    label: error.clone(),
                    icon_name: "dialog-error-symbolic".into(),
                    enabled: false,
                    ..Default::default()
                }
                .into(),
            );
        }

        items.extend([
            ksni::MenuItem::Separator,
            StandardItem {
                label: fl!("tray-settings"),
                icon_name: "preferences-system-symbolic".into(),
                activate: Box::new(|tray: &mut Self| tray.send(TrayEvent::ShowWindow)),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: fl!("tray-quit"),
                icon_name: "application-exit-symbolic".into(),
                activate: Box::new(|tray: &mut Self| tray.send(TrayEvent::Quit)),
                ..Default::default()
            }
            .into(),
        ]);

        items
    }
}
