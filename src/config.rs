// SPDX-License-Identifier: GPL-3.0

use cosmic::cosmic_config::{self, CosmicConfigEntry, cosmic_config_derive::CosmicConfigEntry};

/// Runtime monitoring state, written by the leader applet instance and
/// mirrored by the others (one applet process runs per panel/output).
#[derive(Debug, Default, Clone, CosmicConfigEntry, Eq, PartialEq)]
#[version = 1]
pub struct MonitorState {
    pub is_engaged: bool,
    pub matched_processes: Vec<String>,
    /// Empty string means no error.
    pub last_error: String,
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
