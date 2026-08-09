// SPDX-License-Identifier: GPL-3.0

use crate::client::SpeedLimits;
use reqwest::Client;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Which qBittorrent WebAPI dialect to speak. qBittorrent 5.0 (WebAPI 2.11)
/// renamed the torrents pause/resume endpoints to stop/start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QbitApiVersion {
    V4,
    V5,
}

#[derive(Debug, Clone)]
pub struct QbitClient {
    client: Client,
    base_url: String,
    username: String,
    password: String,
    api_version: QbitApiVersion,
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
    pub fn new(
        base_url: &str,
        username: &str,
        password: &str,
        api_version: QbitApiVersion,
    ) -> Self {
        let base_url = base_url.trim_end_matches('/').to_string();

        // qBittorrent 5.x rejects requests without a Referer/Origin header
        // matching the target host (CSRF protection).
        let mut headers = reqwest::header::HeaderMap::new();
        if let Ok(value) = reqwest::header::HeaderValue::from_str(&base_url) {
            headers.insert(reqwest::header::REFERER, value.clone());
            headers.insert(reqwest::header::ORIGIN, value);
        }

        let client = Client::builder()
            .cookie_store(true)
            .default_headers(headers)
            .build()
            .expect("Failed to build HTTP client");

        Self {
            client,
            base_url,
            username: username.to_string(),
            password: password.to_string(),
            api_version,
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

        let status = response.status();
        let text = response
            .text()
            .await
            .map_err(|e| QbitError::RequestFailed(e.to_string()))?;

        // qBittorrent replies 200 "Ok." on success and 200 "Fails." on bad
        // credentials; some proxy setups strip the body (e.g. 204 No
        // Content), so trust any 2xx that isn't an explicit failure.
        if status.is_success() && !text.contains("Fails") {
            let mut auth = self.authenticated.lock().await;
            *auth = true;
            Ok(())
        } else if !status.is_success() && !text.trim().is_empty() && !text.contains("Fails") {
            // Surface the server's own message (e.g. "Your IP address has
            // been banned after too many failed authentication attempts.").
            Err(QbitError::RequestFailed(text.trim().to_string()))
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

    /// Re-authenticate and retry on 403. Returns the final response status.
    async fn post_with_retry(
        &self,
        url: &str,
        params: &[(&str, &str)],
    ) -> Result<reqwest::StatusCode, QbitError> {
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

            let response = self
                .client
                .post(url)
                .form(params)
                .send()
                .await
                .map_err(|e| QbitError::RequestFailed(e.to_string()))?;
            return Ok(response.status());
        }

        Ok(response.status())
    }

    /// Post to `primary`, falling back to `fallback` if the endpoint does not
    /// exist (safety net when the configured client version doesn't match the
    /// server).
    async fn post_with_fallback(
        &self,
        primary: &str,
        fallback: &str,
        params: &[(&str, &str)],
    ) -> Result<(), QbitError> {
        let status = self.post_with_retry(primary, params).await?;
        if status == reqwest::StatusCode::NOT_FOUND {
            self.post_with_retry(fallback, params).await?;
        }
        Ok(())
    }

    pub async fn pause_all(&self) -> Result<(), QbitError> {
        self.ensure_authenticated().await?;
        // qBittorrent 5.x (WebAPI >= 2.11) renamed "pause" to "stop".
        let stop_url = format!("{}/api/v2/torrents/stop", self.base_url);
        let pause_url = format!("{}/api/v2/torrents/pause", self.base_url);
        let (primary, fallback) = match self.api_version {
            QbitApiVersion::V5 => (&stop_url, &pause_url),
            QbitApiVersion::V4 => (&pause_url, &stop_url),
        };
        self.post_with_fallback(primary, fallback, &[("hashes", "all")])
            .await
    }

    pub async fn resume_all(&self) -> Result<(), QbitError> {
        self.ensure_authenticated().await?;
        // qBittorrent 5.x (WebAPI >= 2.11) renamed "resume" to "start".
        let start_url = format!("{}/api/v2/torrents/start", self.base_url);
        let resume_url = format!("{}/api/v2/torrents/resume", self.base_url);
        let (primary, fallback) = match self.api_version {
            QbitApiVersion::V5 => (&start_url, &resume_url),
            QbitApiVersion::V4 => (&resume_url, &start_url),
        };
        self.post_with_fallback(primary, fallback, &[("hashes", "all")])
            .await
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
        self.post_with_retry(&url, &[("limit", &limit_str)])
            .await
            .map(|_| ())
    }

    /// Set the global upload speed limit (bytes/sec, 0 = unlimited).
    pub async fn set_upload_limit(&self, limit: u64) -> Result<(), QbitError> {
        self.ensure_authenticated().await?;
        let url = format!("{}/api/v2/transfer/setUploadLimit", self.base_url);
        let limit_str = limit.to_string();
        self.post_with_retry(&url, &[("limit", &limit_str)])
            .await
            .map(|_| ())
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
