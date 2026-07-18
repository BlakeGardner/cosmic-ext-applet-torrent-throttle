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
