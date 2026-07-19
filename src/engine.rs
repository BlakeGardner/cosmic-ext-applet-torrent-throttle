// SPDX-License-Identifier: GPL-3.0

//! Shared monitoring engine: checks processes and engages/disengages
//! qBittorrent, independent of any particular UI.

use crate::config::{ActionMode, Config};
use crate::monitor::ProcessMonitor;
use crate::qbit::{QbitClient, SpeedLimits};

#[derive(Debug, Clone, PartialEq)]
pub enum ActionTaken {
    Paused,
    Resumed,
    Throttled,
    Unthrottled,
    None,
}

#[derive(Debug, Clone)]
pub struct MonitorResult {
    pub matched_processes: Vec<String>,
    pub action_taken: ActionTaken,
    pub saved_limits: Option<SpeedLimits>,
    pub error: Option<String>,
}

/// Run one monitoring cycle: scan processes and engage/disengage as needed.
pub async fn run_cycle(
    config: Config,
    was_engaged: bool,
    saved_limits: Option<SpeedLimits>,
) -> MonitorResult {
    let mut monitor = ProcessMonitor::new();
    let matched = monitor.check_patterns(&config.patterns);
    let has_matches = !matched.is_empty();

    let (action, new_saved_limits, error) = if has_matches && !was_engaged {
        if config.qbit_url.trim().is_empty() {
            // Nothing to engage against; surface a clear hint instead of a
            // cryptic HTTP client error.
            (
                ActionTaken::None,
                None,
                Some("qBittorrent URL not configured — open Settings".to_string()),
            )
        } else {
            engage(&config).await
        }
    } else if !has_matches && was_engaged {
        let (action, error) = disengage(&config, saved_limits).await;
        (action, None, error)
    } else {
        (ActionTaken::None, None, None)
    };

    MonitorResult {
        matched_processes: matched,
        action_taken: action,
        saved_limits: new_saved_limits,
        error,
    }
}

async fn engage(config: &Config) -> (ActionTaken, Option<SpeedLimits>, Option<String>) {
    let client = QbitClient::new(
        &config.qbit_url,
        &config.qbit_username,
        &config.qbit_password,
    );
    match config.action_mode {
        ActionMode::Pause => match client.pause_all().await {
            Ok(()) => (ActionTaken::Paused, None, None),
            Err(e) => (ActionTaken::None, None, Some(e.to_string())),
        },
        ActionMode::Throttle => {
            // Save current limits before applying the throttle.
            match client.get_speed_limits().await {
                Ok(current) => {
                    let target = SpeedLimits {
                        download: config.throttle_download_kbps * 1024,
                        upload: config.throttle_upload_kbps * 1024,
                    };
                    match client.set_speed_limits(&target).await {
                        Ok(()) => (ActionTaken::Throttled, Some(current), None),
                        Err(e) => (ActionTaken::None, None, Some(e.to_string())),
                    }
                }
                Err(e) => (ActionTaken::None, None, Some(e.to_string())),
            }
        }
    }
}

/// Resume torrents or restore speed limits depending on the action mode.
pub async fn disengage(
    config: &Config,
    saved_limits: Option<SpeedLimits>,
) -> (ActionTaken, Option<String>) {
    let client = QbitClient::new(
        &config.qbit_url,
        &config.qbit_username,
        &config.qbit_password,
    );
    match config.action_mode {
        ActionMode::Pause => match client.resume_all().await {
            Ok(()) => (ActionTaken::Resumed, None),
            Err(e) => (ActionTaken::None, Some(e.to_string())),
        },
        ActionMode::Throttle => {
            let restore = saved_limits.unwrap_or(SpeedLimits {
                download: 0,
                upload: 0,
            });
            match client.set_speed_limits(&restore).await {
                Ok(()) => (ActionTaken::Unthrottled, None),
                Err(e) => (ActionTaken::None, Some(e.to_string())),
            }
        }
    }
}
