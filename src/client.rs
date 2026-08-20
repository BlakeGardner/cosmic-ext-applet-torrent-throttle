// SPDX-License-Identifier: GPL-3.0

//! Torrent client abstraction. All UI and engine code talks to
//! [`TorrentClient`]; adding support for a new torrent client means adding a
//! variant here (plus a [`ClientKind`](crate::config::ClientKind) entry) and
//! implementing the same set of operations.

use crate::config::{ClientKind, Config};
use crate::qbit::{QbitApiVersion, QbitClient};
use crate::transmission::TransmissionClient;

/// Stored speed limits (bytes/sec). 0 means unlimited.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpeedLimits {
    pub download: u64,
    pub upload: u64,
}

/// A handle to whichever torrent client the user configured.
#[derive(Debug, Clone)]
pub enum TorrentClient {
    Qbit(QbitClient),
    Transmission(TransmissionClient),
}

impl TorrentClient {
    /// Build the concrete client selected in the configuration.
    pub fn from_config(config: &Config) -> Self {
        match config.client_kind {
            ClientKind::QbitV5 | ClientKind::QbitV4 => {
                let api_version = match config.client_kind {
                    ClientKind::QbitV4 => QbitApiVersion::V4,
                    _ => QbitApiVersion::V5,
                };
                TorrentClient::Qbit(QbitClient::new(
                    &config.qbit_url,
                    &config.qbit_username,
                    &config.qbit_password,
                    api_version,
                ))
            }
            ClientKind::Transmission => TorrentClient::Transmission(TransmissionClient::new(
                &config.qbit_url,
                &config.qbit_username,
                &config.qbit_password,
            )),
        }
    }

    pub async fn pause_all(&self) -> Result<(), String> {
        match self {
            TorrentClient::Qbit(c) => c.pause_all().await.map_err(|e| e.to_string()),
            TorrentClient::Transmission(c) => c.pause_all().await.map_err(|e| e.to_string()),
        }
    }

    pub async fn resume_all(&self) -> Result<(), String> {
        match self {
            TorrentClient::Qbit(c) => c.resume_all().await.map_err(|e| e.to_string()),
            TorrentClient::Transmission(c) => c.resume_all().await.map_err(|e| e.to_string()),
        }
    }

    pub async fn get_speed_limits(&self) -> Result<SpeedLimits, String> {
        match self {
            TorrentClient::Qbit(c) => c.get_speed_limits().await.map_err(|e| e.to_string()),
            TorrentClient::Transmission(c) => {
                c.get_speed_limits().await.map_err(|e| e.to_string())
            }
        }
    }

    pub async fn set_speed_limits(&self, limits: &SpeedLimits) -> Result<(), String> {
        match self {
            TorrentClient::Qbit(c) => c.set_speed_limits(limits).await.map_err(|e| e.to_string()),
            TorrentClient::Transmission(c) => {
                c.set_speed_limits(limits).await.map_err(|e| e.to_string())
            }
        }
    }

    /// Test connectivity and return a human-readable client description,
    /// e.g. "qBittorrent v5.0.2".
    pub async fn test_connection(&self) -> Result<String, String> {
        match self {
            TorrentClient::Qbit(c) => c
                .test_connection()
                .await
                .map(|version| format!("qBittorrent {}", version.trim()))
                .map_err(|e| e.to_string()),
            TorrentClient::Transmission(c) => c
                .test_connection()
                .await
                .map(|version| format!("Transmission {}", version.trim()))
                .map_err(|e| e.to_string()),
        }
    }
}
