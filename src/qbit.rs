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

    /// Whether the user left the credentials blank, meaning the WebUI is
    /// expected to allow unauthenticated access (e.g. "Bypass authentication
    /// for clients on localhost").
    fn credentials_blank(&self) -> bool {
        self.username.is_empty() && self.password.is_empty()
    }

    /// The error to report when the server refuses a request with 403.
    fn auth_required_error(&self) -> QbitError {
        if self.credentials_blank() {
            QbitError::RequestFailed(
                "authentication required: enter credentials, or enable \
                 \"Bypass authentication for clients on localhost\" in \
                 qBittorrent's WebUI settings"
                    .to_string(),
            )
        } else {
            QbitError::AuthFailed
        }
    }

    /// The error for a non-success status that isn't 403 — typically a
    /// different service answering on the configured URL (e.g. Transmission
    /// replying 409) or a reverse-proxy error page.
    fn unexpected_status_error(&self, status: reqwest::StatusCode, url: &str) -> QbitError {
        QbitError::RequestFailed(format!(
            "HTTP {status} from {url} — check that the URL points to a qBittorrent \
             WebUI and the right client is selected in Settings"
        ))
    }

    async fn login(&self) -> Result<(), QbitError> {
        // With blank credentials there is nothing to log in with; requests
        // are sent without a session and rely on the WebUI's authentication
        // bypass. A 403 response later reports a helpful error instead.
        if self.credentials_blank() {
            let mut auth = self.authenticated.lock().await;
            *auth = true;
            return Ok(());
        }

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

        let response = if response.status() == reqwest::StatusCode::FORBIDDEN {
            let mut auth = self.authenticated.lock().await;
            *auth = false;
            drop(auth);
            self.login().await?;

            self.client
                .post(url)
                .form(params)
                .send()
                .await
                .map_err(|e| QbitError::RequestFailed(e.to_string()))?
        } else {
            response
        };

        if response.status() == reqwest::StatusCode::FORBIDDEN {
            return Err(self.auth_required_error());
        }

        Ok(response.status())
    }

    /// GET returning the response body, re-authenticating and retrying on 403.
    async fn get_with_retry(&self, url: &str) -> Result<String, QbitError> {
        let response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|e| QbitError::RequestFailed(e.to_string()))?;

        let response = if response.status() == reqwest::StatusCode::FORBIDDEN {
            let mut auth = self.authenticated.lock().await;
            *auth = false;
            drop(auth);
            self.login().await?;

            self.client
                .get(url)
                .send()
                .await
                .map_err(|e| QbitError::RequestFailed(e.to_string()))?
        } else {
            response
        };

        if response.status() == reqwest::StatusCode::FORBIDDEN {
            return Err(self.auth_required_error());
        }
        if !response.status().is_success() {
            // e.g. a Transmission server answering 409, or a reverse proxy
            // error page — anything that is not the qBittorrent WebAPI.
            return Err(self.unexpected_status_error(response.status(), url));
        }

        response
            .text()
            .await
            .map_err(|e| QbitError::RequestFailed(e.to_string()))
    }

    /// Post to `primary`, falling back to `fallback` if the endpoint does not
    /// exist (safety net when the configured client version doesn't match the
    /// server). Any other non-success status is an error.
    async fn post_with_fallback(
        &self,
        primary: &str,
        fallback: &str,
        params: &[(&str, &str)],
    ) -> Result<(), QbitError> {
        let mut status = self.post_with_retry(primary, params).await?;
        let mut url = primary;
        if status == reqwest::StatusCode::NOT_FOUND {
            status = self.post_with_retry(fallback, params).await?;
            url = fallback;
        }
        if !status.is_success() {
            return Err(self.unexpected_status_error(status, url));
        }
        Ok(())
    }

    /// POST and require a success status.
    async fn post_expect_success(
        &self,
        url: &str,
        params: &[(&str, &str)],
    ) -> Result<(), QbitError> {
        let status = self.post_with_retry(url, params).await?;
        if !status.is_success() {
            return Err(self.unexpected_status_error(status, url));
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
        let text = self.get_with_retry(&url).await?;

        text.trim()
            .parse::<u64>()
            .map_err(|e| QbitError::RequestFailed(format!("failed to parse download limit: {e}")))
    }

    /// Get the current global upload speed limit (bytes/sec, 0 = unlimited).
    pub async fn get_upload_limit(&self) -> Result<u64, QbitError> {
        self.ensure_authenticated().await?;
        let url = format!("{}/api/v2/transfer/uploadLimit", self.base_url);
        let text = self.get_with_retry(&url).await?;

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
        self.post_expect_success(&url, &[("limit", &limit_str)])
            .await
    }

    /// Set the global upload speed limit (bytes/sec, 0 = unlimited).
    pub async fn set_upload_limit(&self, limit: u64) -> Result<(), QbitError> {
        self.ensure_authenticated().await?;
        let url = format!("{}/api/v2/transfer/setUploadLimit", self.base_url);
        let limit_str = limit.to_string();
        self.post_expect_success(&url, &[("limit", &limit_str)])
            .await
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
        self.get_with_retry(&url).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_http::mock_server;

    /// A Transmission server answers qBittorrent API requests with 409 and an
    /// HTML body. That must surface as a clear error, not a number-parse
    /// failure on the HTML ("invalid digit found in string").
    #[tokio::test]
    async fn get_limit_rejects_non_success_status() {
        let (url, _server) = mock_server(vec![(
            "409 Conflict",
            "Content-Type: text/html\r\n",
            "<h1>409: Conflict</h1><p>invalid session_id header.</p>",
        )])
        .await;

        let client = QbitClient::new(&url, "", "", QbitApiVersion::V5);
        let err = client.get_download_limit().await.unwrap_err();
        match err {
            QbitError::RequestFailed(msg) => {
                assert!(msg.contains("HTTP 409"), "unexpected message: {msg}");
                assert!(!msg.contains("invalid digit"), "unexpected message: {msg}");
            }
            other => panic!("expected RequestFailed, got {other:?}"),
        }
    }

    /// pause_all against a non-qBittorrent server must fail, not silently
    /// succeed (only 404 triggers the version fallback).
    #[tokio::test]
    async fn pause_all_rejects_non_success_status() {
        let (url, _server) = mock_server(vec![(
            "409 Conflict",
            "Content-Type: text/html\r\n",
            "<h1>409: Conflict</h1>",
        )])
        .await;

        let client = QbitClient::new(&url, "", "", QbitApiVersion::V5);
        let err = client.pause_all().await.unwrap_err();
        match err {
            QbitError::RequestFailed(msg) => {
                assert!(msg.contains("HTTP 409"), "unexpected message: {msg}")
            }
            other => panic!("expected RequestFailed, got {other:?}"),
        }
    }

    /// set limits must also report non-success statuses.
    #[tokio::test]
    async fn set_limit_rejects_non_success_status() {
        let (url, _server) =
            mock_server(vec![("500 Internal Server Error", "", "something broke")]).await;

        let client = QbitClient::new(&url, "", "", QbitApiVersion::V5);
        let err = client.set_download_limit(1024).await.unwrap_err();
        match err {
            QbitError::RequestFailed(msg) => {
                assert!(msg.contains("HTTP 500"), "unexpected message: {msg}")
            }
            other => panic!("expected RequestFailed, got {other:?}"),
        }
    }

    /// The 404 endpoint-rename fallback still works: stop -> 404, pause -> 200.
    #[tokio::test]
    async fn pause_all_falls_back_on_404() {
        let (url, server) =
            mock_server(vec![("404 Not Found", "", ""), ("200 OK", "", "Ok.")]).await;

        let client = QbitClient::new(&url, "", "", QbitApiVersion::V5);
        client.pause_all().await.unwrap();

        let requests = server.await.unwrap();
        assert!(requests[0].starts_with("POST /api/v2/torrents/stop"));
        assert!(requests[1].starts_with("POST /api/v2/torrents/pause"));
    }

    /// Happy path: limits parse from plain numeric bodies.
    #[tokio::test]
    async fn get_speed_limits_parses_numbers() {
        let (url, _server) =
            mock_server(vec![("200 OK", "", "1048576"), ("200 OK", "", "0")]).await;

        let client = QbitClient::new(&url, "", "", QbitApiVersion::V5);
        let limits = client.get_speed_limits().await.unwrap();
        assert_eq!(
            limits,
            crate::client::SpeedLimits {
                download: 1_048_576,
                upload: 0,
            }
        );
    }
}
