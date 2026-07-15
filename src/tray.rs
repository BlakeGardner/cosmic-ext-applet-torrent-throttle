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
    pub tx: UnboundedSender<TrayEvent>,
}

impl QbitTray {
    fn send(&self, event: TrayEvent) {
        let _ = self.tx.unbounded_send(event);
    }
}

impl ksni::Tray for QbitTray {
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
        ksni::ToolTip {
            title: fl!("app-title"),
            description,
            ..Default::default()
        }
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        self.send(TrayEvent::ShowWindow);
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::{CheckmarkItem, StandardItem};

        let mut items: Vec<ksni::MenuItem<Self>> = vec![
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

        items.extend([
            ksni::MenuItem::Separator,
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
                label: fl!("tray-show-window"),
                icon_name: "window-new-symbolic".into(),
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
