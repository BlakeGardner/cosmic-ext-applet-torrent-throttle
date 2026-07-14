// SPDX-License-Identifier: GPL-3.0

use cosmic::cosmic_config::{self, CosmicConfigEntry, cosmic_config_derive::CosmicConfigEntry};

#[derive(Debug, Default, Clone, CosmicConfigEntry, Eq, PartialEq)]
#[version = 1]
pub struct Config {
    pub qbit_url: String,
    pub qbit_username: String,
    pub qbit_password: String,
    pub patterns: Vec<String>,
    pub poll_interval_secs: u64,
    pub enabled: bool,
}
