// SPDX-License-Identifier: GPL-3.0

use cosmic::cosmic_config::{self, cosmic_config_derive::CosmicConfigEntry, CosmicConfigEntry};

/// Runtime monitoring state, written by the leader applet instance and
/// mirrored by the others (one applet process runs per panel/output).
#[derive(Debug, Default, Clone, CosmicConfigEntry, Eq, PartialEq)]
#[version = 1]
pub struct MonitorState {
    pub is_engaged: bool,
    pub matched_processes: Vec<String>,
    /// Empty string means no error.
    pub last_error: String,
    /// Whether the saved speed limits below are meaningful. Persisted so the
    /// original limits survive an applet restart while a throttle is engaged.
    pub has_saved_limits: bool,
    /// Speed limits (bytes/sec) captured before the throttle was applied.
    pub saved_download_limit: u64,
    pub saved_upload_limit: u64,
}

/// Quit broadcast, written by whichever applet instance the user quit from
/// and watched by all instances (SIGTERM cannot cross Flatpak sandboxes).
#[derive(Debug, Default, Clone, CosmicConfigEntry, Eq, PartialEq)]
#[version = 1]
pub struct QuitSignal {
    /// Milliseconds since the Unix epoch; instances started before this
    /// moment quit when they observe it.
    pub quit_at_millis: u64,
}

/// Which torrent client (and API dialect) to talk to. New torrent clients
/// are added here and in [`TorrentClient`](crate::client::TorrentClient).
#[derive(Debug, Default, Clone, Copy, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ClientKind {
    /// qBittorrent 5.x (WebAPI 2.11+).
    #[default]
    QbitV5,
    /// qBittorrent 4.x.
    QbitV4,
}

impl ClientKind {
    /// All selectable client kinds, in the order shown in the GUI.
    pub const ALL: &'static [ClientKind] = &[ClientKind::QbitV5, ClientKind::QbitV4];

    /// Display labels matching [`ClientKind::ALL`] (product names, not localized).
    pub const LABELS: &'static [&'static str] = &["qBittorrent 5.x", "qBittorrent 4.x"];

    /// Position within [`ClientKind::ALL`], for dropdown selection state.
    pub fn index(&self) -> usize {
        Self::ALL.iter().position(|k| k == self).unwrap_or(0)
    }
}

/// What action to take when a matching process is detected.
#[derive(Debug, Default, Clone, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ActionMode {
    /// Pause all torrents.
    #[default]
    Pause,
    /// Set a global speed throttle (KB/s). 0 means unlimited.
    Throttle,
}

#[derive(Debug, Default, Clone, CosmicConfigEntry, Eq, PartialEq)]
#[version = 1]
pub struct Config {
    /// Which torrent client to control.
    pub client_kind: ClientKind,
    pub qbit_url: String,
    pub qbit_username: String,
    pub qbit_password: String,
    pub patterns: Vec<String>,
    pub poll_interval_secs: u64,
    pub enabled: bool,
    /// Whether to pause or throttle when a match is detected.
    pub action_mode: ActionMode,
    /// Download speed limit in KB/s to apply when throttling (0 = unlimited).
    pub throttle_download_kbps: u64,
    /// Upload speed limit in KB/s to apply when throttling (0 = unlimited).
    pub throttle_upload_kbps: u64,
}
