// SPDX-License-Identifier: GPL-3.0

use reqwest::Client;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Stored speed limits (bytes/sec). 0 means unlimited.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpeedLimits {
    pub download: u64,
    pub upload: u64,
}

#[derive(Debug, Clone)]
pub struct QbitClient {
    client: Client,
    base_url: String,
    username: String,
    password: String,
    authenticated: Arc<Mutex<bool>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum QbitError {
    AuthFailed,
    RequestFailed(String),
}

impl std::fmt::Display for QbitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            QbitError::AuthFailed => write!(f, "Authentication failed"),
            QbitError::RequestFailed(msg) => write!(f, "Request failed: {}", msg),
        }
    }
}

impl QbitClient {
    pub fn new(base_url: &str, username: &str, password: &str) -> Self {
        let client = Client::builder()
            .cookie_store(true)
            .build()
            .expect("Failed to build HTTP client");

        Self {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
            username: username.to_string(),
            password: password.to_string(),
            authenticated: Arc::new(Mutex::new(false)),
        }
    }

    async fn login(&self) -> Result<(), QbitError> {
        let url = format!("{}/api/v2/auth/login", self.base_url);
        let params = [
            ("username", self.username.as_str()),
            ("password", self.password.as_str()),
        ];

        let response = self
            .client
            .post(&url)
            .form(&params)
            .send()
            .await
            .map_err(|e| QbitError::RequestFailed(e.to_string()))?;

        let text = response
            .text()
            .await
            .map_err(|e| QbitError::RequestFailed(e.to_string()))?;

        if text.contains("Ok") {
            let mut auth = self.authenticated.lock().await;
            *auth = true;
            Ok(())
        } else {
            Err(QbitError::AuthFailed)
        }
    }

    async fn ensure_authenticated(&self) -> Result<(), QbitError> {
        let auth = self.authenticated.lock().await;
        if !*auth {
            drop(auth);
            self.login().await?;
        }
        Ok(())
    }

    /// Re-authenticate and retry on 403.
    async fn post_with_retry(&self, url: &str, params: &[(&str, &str)]) -> Result<(), QbitError> {
        let response = self
            .client
            .post(url)
            .form(params)
            .send()
            .await
            .map_err(|e| QbitError::RequestFailed(e.to_string()))?;

        if response.status() == reqwest::StatusCode::FORBIDDEN {
            let mut auth = self.authenticated.lock().await;
            *auth = false;
            drop(auth);
            self.login().await?;

            self.client
                .post(url)
                .form(params)
                .send()
                .await
                .map_err(|e| QbitError::RequestFailed(e.to_string()))?;
        }

        Ok(())
    }

    pub async fn pause_all(&self) -> Result<(), QbitError> {
        self.ensure_authenticated().await?;
        let url = format!("{}/api/v2/torrents/pause", self.base_url);
        self.post_with_retry(&url, &[("hashes", "all")]).await
    }

    pub async fn resume_all(&self) -> Result<(), QbitError> {
        self.ensure_authenticated().await?;
        let url = format!("{}/api/v2/torrents/resume", self.base_url);
        self.post_with_retry(&url, &[("hashes", "all")]).await
    }

    /// Get the current global download speed limit (bytes/sec, 0 = unlimited).
    pub async fn get_download_limit(&self) -> Result<u64, QbitError> {
        self.ensure_authenticated().await?;
        let url = format!("{}/api/v2/transfer/downloadLimit", self.base_url);
        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| QbitError::RequestFailed(e.to_string()))?;

        let text = response
            .text()
            .await
            .map_err(|e| QbitError::RequestFailed(e.to_string()))?;

        text.trim()
            .parse::<u64>()
            .map_err(|e| QbitError::RequestFailed(format!("failed to parse download limit: {e}")))
    }

    /// Get the current global upload speed limit (bytes/sec, 0 = unlimited).
    pub async fn get_upload_limit(&self) -> Result<u64, QbitError> {
        self.ensure_authenticated().await?;
        let url = format!("{}/api/v2/transfer/uploadLimit", self.base_url);
        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| QbitError::RequestFailed(e.to_string()))?;

        let text = response
            .text()
            .await
            .map_err(|e| QbitError::RequestFailed(e.to_string()))?;

        text.trim()
            .parse::<u64>()
            .map_err(|e| QbitError::RequestFailed(format!("failed to parse upload limit: {e}")))
    }

    /// Get both current speed limits.
    pub async fn get_speed_limits(&self) -> Result<SpeedLimits, QbitError> {
        let download = self.get_download_limit().await?;
        let upload = self.get_upload_limit().await?;
        Ok(SpeedLimits { download, upload })
    }

    /// Set the global download speed limit (bytes/sec, 0 = unlimited).
    pub async fn set_download_limit(&self, limit: u64) -> Result<(), QbitError> {
        self.ensure_authenticated().await?;
        let url = format!("{}/api/v2/transfer/setDownloadLimit", self.base_url);
        let limit_str = limit.to_string();
        self.post_with_retry(&url, &[("limit", &limit_str)]).await
    }

    /// Set the global upload speed limit (bytes/sec, 0 = unlimited).
    pub async fn set_upload_limit(&self, limit: u64) -> Result<(), QbitError> {
        self.ensure_authenticated().await?;
        let url = format!("{}/api/v2/transfer/setUploadLimit", self.base_url);
        let limit_str = limit.to_string();
        self.post_with_retry(&url, &[("limit", &limit_str)]).await
    }

    /// Set both speed limits at once (bytes/sec, 0 = unlimited).
    pub async fn set_speed_limits(&self, limits: &SpeedLimits) -> Result<(), QbitError> {
        self.set_download_limit(limits.download).await?;
        self.set_upload_limit(limits.upload).await?;
        Ok(())
    }

    pub async fn test_connection(&self) -> Result<String, QbitError> {
        self.login().await?;

        let url = format!("{}/api/v2/app/version", self.base_url);
        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| QbitError::RequestFailed(e.to_string()))?;

        response
            .text()
            .await
            .map_err(|e| QbitError::RequestFailed(e.to_string()))
    }
}
