use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub qbit_url: String,
    pub qbit_username: String,
    pub qbit_password: String,
    pub patterns: Vec<String>,
    pub poll_interval_secs: u64,
    pub enabled: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            qbit_url: "http://localhost:8080".to_string(),
            qbit_username: "admin".to_string(),
            qbit_password: String::new(),
            patterns: Vec::new(),
            poll_interval_secs: 30,
            enabled: true,
        }
    }
}

impl AppConfig {
    pub fn config_path() -> PathBuf {
        let config_dir = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("cosmic-qbit-remote");
        fs::create_dir_all(&config_dir).ok();
        config_dir.join("config.json")
    }

    pub fn load() -> Self {
        let path = Self::config_path();
        if path.exists() {
            let content = fs::read_to_string(&path).unwrap_or_default();
            serde_json::from_str(&content).unwrap_or_default()
        } else {
            let config = Self::default();
            config.save();
            config
        }
    }

    pub fn save(&self) {
        let path = Self::config_path();
        if let Ok(content) = serde_json::to_string_pretty(self) {
            fs::write(path, content).ok();
        }
    }
}
