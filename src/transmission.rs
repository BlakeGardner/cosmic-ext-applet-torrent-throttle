// SPDX-License-Identifier: GPL-3.0

//! Transmission RPC client. Speaks the JSON-over-HTTP protocol documented in
//! Transmission's rpc-spec: a single endpoint, `{"method", "arguments"}`
//! bodies, HTTP basic auth, and a CSRF handshake where the server replies
//! 409 with an `X-Transmission-Session-Id` header to echo back.

use crate::client::SpeedLimits;
use reqwest::{Client, StatusCode};
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::Mutex;

/// Transmission uses SI units: RPC speed limits are in "KBps" meaning
/// thousands of bytes per second.
const KBPS: u64 = 1000;

const SESSION_ID_HEADER: &str = "X-Transmission-Session-Id";

#[derive(Debug, Clone)]
pub struct TransmissionClient {
    client: Client,
    rpc_url: String,
    username: String,
    password: String,
    /// CSRF token handed out by the server via a 409 response.
    session_id: Arc<Mutex<Option<String>>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TransmissionError {
    AuthFailed,
    RequestFailed(String),
}

impl std::fmt::Display for TransmissionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TransmissionError::AuthFailed => write!(f, "Authentication failed"),
            TransmissionError::RequestFailed(msg) => write!(f, "Request failed: {}", msg),
        }
    }
}

impl TransmissionClient {
    pub fn new(base_url: &str, username: &str, password: &str) -> Self {
        let base = base_url.trim_end_matches('/');
        // Accept both a bare address (http://localhost:9091) and a full RPC
        // path (…/transmission/rpc, or a custom …/rpc behind a reverse proxy).
        let rpc_url = if base.ends_with("/rpc") {
            base.to_string()
        } else {
            format!("{}/transmission/rpc", base)
        };

        let client = Client::builder()
            .build()
            .expect("Failed to build HTTP client");

        Self {
            client,
            rpc_url,
            username: username.to_string(),
            password: password.to_string(),
            session_id: Arc::new(Mutex::new(None)),
        }
    }

    /// Whether the user left the credentials blank, meaning the RPC server is
    /// expected to allow unauthenticated access (rpc-authentication-required
    /// disabled, Transmission's default).
    fn credentials_blank(&self) -> bool {
        self.username.is_empty() && self.password.is_empty()
    }

    /// The error to report when the server refuses a request with 401.
    fn auth_required_error(&self) -> TransmissionError {
        if self.credentials_blank() {
            TransmissionError::RequestFailed(
                "authentication required: enter the username and password from \
                 Transmission's remote access settings"
                    .to_string(),
            )
        } else {
            TransmissionError::AuthFailed
        }
    }

    async fn post(&self, body: &Value) -> Result<reqwest::Response, TransmissionError> {
        let mut request = self.client.post(&self.rpc_url).json(body);
        if !self.credentials_blank() {
            request = request.basic_auth(&self.username, Some(&self.password));
        }
        if let Some(id) = self.session_id.lock().await.clone() {
            request = request.header(SESSION_ID_HEADER, id);
        }
        request
            .send()
            .await
            .map_err(|e| TransmissionError::RequestFailed(e.to_string()))
    }

    /// Send one RPC request, transparently performing the 409 session-id
    /// handshake, and return the reply's `arguments` object.
    async fn send(&self, method: &str, arguments: Value) -> Result<Value, TransmissionError> {
        let body = json!({ "method": method, "arguments": arguments });

        let response = self.post(&body).await?;

        // 409 means the session id is missing or stale; the fresh one is in
        // the response headers. Cache it and retry once.
        let response = if response.status() == StatusCode::CONFLICT {
            let fresh_id = response
                .headers()
                .get(SESSION_ID_HEADER)
                .and_then(|v| v.to_str().ok())
                .map(str::to_string)
                .ok_or_else(|| {
                    TransmissionError::RequestFailed(
                        "server sent 409 without a session id header".to_string(),
                    )
                })?;
            *self.session_id.lock().await = Some(fresh_id);
            self.post(&body).await?
        } else {
            response
        };

        let status = response.status();
        match status {
            StatusCode::UNAUTHORIZED => return Err(self.auth_required_error()),
            StatusCode::FORBIDDEN => {
                return Err(TransmissionError::RequestFailed(
                    "forbidden: check Transmission's rpc-whitelist settings".to_string(),
                ))
            }
            StatusCode::CONFLICT => {
                return Err(TransmissionError::RequestFailed(
                    "could not negotiate a session id".to_string(),
                ))
            }
            _ if !status.is_success() => {
                return Err(TransmissionError::RequestFailed(format!("HTTP {}", status)))
            }
            _ => {}
        }

        let reply: Value = response
            .json()
            .await
            .map_err(|e| TransmissionError::RequestFailed(format!("invalid RPC reply: {e}")))?;

        // Transmission reports errors in-band: "result" is "success" or a
        // human-readable error string.
        let result = reply.get("result").and_then(Value::as_str).unwrap_or("");
        if result != "success" {
            return Err(TransmissionError::RequestFailed(if result.is_empty() {
                "RPC reply missing result field".to_string()
            } else {
                result.to_string()
            }));
        }

        Ok(reply.get("arguments").cloned().unwrap_or(Value::Null))
    }

    pub async fn pause_all(&self) -> Result<(), TransmissionError> {
        // Omitting the "ids" argument applies the request to all torrents.
        self.send("torrent-stop", json!({})).await.map(|_| ())
    }

    pub async fn resume_all(&self) -> Result<(), TransmissionError> {
        self.send("torrent-start", json!({})).await.map(|_| ())
    }

    /// Get both global speed limits (bytes/sec, 0 = unlimited). Transmission
    /// models "unlimited" as a disabled limit rather than a zero value.
    pub async fn get_speed_limits(&self) -> Result<SpeedLimits, TransmissionError> {
        let args = self
            .send(
                "session-get",
                json!({
                    "fields": [
                        "speed-limit-down",
                        "speed-limit-down-enabled",
                        "speed-limit-up",
                        "speed-limit-up-enabled",
                    ]
                }),
            )
            .await?;

        let read_limit = |field: &str| -> Result<u64, TransmissionError> {
            let enabled = args
                .get(format!("{field}-enabled"))
                .and_then(Value::as_bool)
                .ok_or_else(|| {
                    TransmissionError::RequestFailed(format!(
                        "session-get reply missing {field}-enabled"
                    ))
                })?;
            if !enabled {
                return Ok(0);
            }
            let kbps = args.get(field).and_then(Value::as_u64).ok_or_else(|| {
                TransmissionError::RequestFailed(format!("session-get reply missing {field}"))
            })?;
            Ok(kbps * KBPS)
        };

        Ok(SpeedLimits {
            download: read_limit("speed-limit-down")?,
            upload: read_limit("speed-limit-up")?,
        })
    }

    /// Set both global speed limits (bytes/sec, 0 = unlimited). A zero limit
    /// disables the limit while leaving the value stored in Transmission
    /// untouched.
    pub async fn set_speed_limits(&self, limits: &SpeedLimits) -> Result<(), TransmissionError> {
        let mut args = serde_json::Map::new();
        args.insert(
            "speed-limit-down-enabled".to_string(),
            json!(limits.download > 0),
        );
        if limits.download > 0 {
            // Round up to at least 1 KBps so a small nonzero limit never
            // becomes a fully-stalled 0 KBps limit.
            args.insert(
                "speed-limit-down".to_string(),
                json!((limits.download / KBPS).max(1)),
            );
        }
        args.insert(
            "speed-limit-up-enabled".to_string(),
            json!(limits.upload > 0),
        );
        if limits.upload > 0 {
            args.insert(
                "speed-limit-up".to_string(),
                json!((limits.upload / KBPS).max(1)),
            );
        }

        self.send("session-set", Value::Object(args))
            .await
            .map(|_| ())
    }

    /// Test connectivity and return the server version, e.g. "4.0.6".
    pub async fn test_connection(&self) -> Result<String, TransmissionError> {
        let args = self
            .send("session-get", json!({ "fields": ["version"] }))
            .await?;
        let version = args
            .get("version")
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or("");
        if version.is_empty() {
            return Err(TransmissionError::RequestFailed(
                "session-get reply missing version".to_string(),
            ));
        }
        Ok(version.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    /// Minimal one-request-per-connection HTTP server. Serves the given
    /// (status line, extra header lines, body) responses in order and returns
    /// the raw captured requests. Every response carries `Connection: close`
    /// so reqwest reconnects for each request.
    async fn mock_server(
        responses: Vec<(&'static str, &'static str, &'static str)>,
    ) -> (String, tokio::task::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            let mut captured = Vec::new();
            for (status, extra_headers, body) in responses {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut buf = vec![0u8; 65536];
                let mut read = 0;
                let request = loop {
                    let n = stream.read(&mut buf[read..]).await.unwrap();
                    read += n;
                    let text = String::from_utf8_lossy(&buf[..read]).to_string();
                    if let Some(header_end) = text.find("\r\n\r\n") {
                        let content_length = text
                            .lines()
                            .find_map(|line| {
                                let (name, value) = line.split_once(':')?;
                                if name.eq_ignore_ascii_case("content-length") {
                                    value.trim().parse::<usize>().ok()
                                } else {
                                    None
                                }
                            })
                            .unwrap_or(0);
                        if read >= header_end + 4 + content_length {
                            break text;
                        }
                    }
                    if n == 0 {
                        break text;
                    }
                };
                captured.push(request);
                let response = format!(
                    "HTTP/1.1 {status}\r\nConnection: close\r\nContent-Length: {}\r\n{extra_headers}\r\n{body}",
                    body.len(),
                );
                stream.write_all(response.as_bytes()).await.unwrap();
                stream.shutdown().await.unwrap();
            }
            captured
        });
        (format!("http://{}", addr), handle)
    }

    const SUCCESS_EMPTY: &str = r#"{"result":"success","arguments":{}}"#;

    #[tokio::test]
    async fn session_id_handshake_and_pause_all() {
        let (url, server) = mock_server(vec![
            ("409 Conflict", "X-Transmission-Session-Id: abc123\r\n", ""),
            ("200 OK", "", SUCCESS_EMPTY),
        ])
        .await;

        let client = TransmissionClient::new(&url, "", "");
        client.pause_all().await.unwrap();

        let requests = server.await.unwrap();
        assert_eq!(requests.len(), 2);
        // Bare URL gets the default RPC path appended.
        assert!(requests[0].starts_with("POST /transmission/rpc HTTP/1.1"));
        // First request has no session id; the retry echoes the fresh one.
        assert!(!requests[0].to_lowercase().contains("x-transmission-session-id"));
        assert!(requests[1]
            .to_lowercase()
            .contains("x-transmission-session-id: abc123"));
        // The retry re-sends the same RPC body.
        assert!(requests[1].contains(r#""method":"torrent-stop""#));
        // Omitted ids = all torrents.
        assert!(!requests[1].contains("ids"));
    }

    #[tokio::test]
    async fn custom_rpc_path_is_preserved() {
        let (url, server) = mock_server(vec![("200 OK", "", SUCCESS_EMPTY)]).await;

        let client = TransmissionClient::new(&format!("{url}/tr/rpc/"), "", "");
        client.resume_all().await.unwrap();

        let requests = server.await.unwrap();
        assert!(requests[0].starts_with("POST /tr/rpc HTTP/1.1"));
        assert!(requests[0].contains(r#""method":"torrent-start""#));
    }

    #[tokio::test]
    async fn get_speed_limits_converts_units() {
        // Download limited to 100 KBps (SI), upload unlimited (disabled).
        let (url, server) = mock_server(vec![(
            "200 OK",
            "",
            r#"{"result":"success","arguments":{
                "speed-limit-down":100,"speed-limit-down-enabled":true,
                "speed-limit-up":50,"speed-limit-up-enabled":false}}"#,
        )])
        .await;

        let client = TransmissionClient::new(&url, "", "");
        let limits = client.get_speed_limits().await.unwrap();
        server.await.unwrap();

        assert_eq!(
            limits,
            SpeedLimits {
                download: 100_000,
                upload: 0,
            }
        );
    }

    #[tokio::test]
    async fn set_speed_limits_converts_and_disables() {
        let (url, server) = mock_server(vec![("200 OK", "", SUCCESS_EMPTY)]).await;

        let client = TransmissionClient::new(&url, "", "");
        // 102400 bytes/sec -> 102 KBps enabled; 0 -> disabled, value untouched.
        client
            .set_speed_limits(&SpeedLimits {
                download: 102_400,
                upload: 0,
            })
            .await
            .unwrap();

        let requests = server.await.unwrap();
        let body = &requests[0];
        assert!(body.contains(r#""method":"session-set""#));
        assert!(body.contains(r#""speed-limit-down":102"#));
        assert!(body.contains(r#""speed-limit-down-enabled":true"#));
        assert!(body.contains(r#""speed-limit-up-enabled":false"#));
        assert!(!body.contains(r#""speed-limit-up":"#));
    }

    #[tokio::test]
    async fn blank_credentials_get_helpful_auth_error() {
        let (url, _server) = mock_server(vec![("401 Unauthorized", "", "")]).await;

        let client = TransmissionClient::new(&url, "", "");
        let err = client.pause_all().await.unwrap_err();
        match err {
            TransmissionError::RequestFailed(msg) => {
                assert!(msg.contains("authentication required"))
            }
            other => panic!("expected RequestFailed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn wrong_credentials_report_auth_failed() {
        let (url, server) = mock_server(vec![("401 Unauthorized", "", "")]).await;

        let client = TransmissionClient::new(&url, "user", "wrong");
        let err = client.pause_all().await.unwrap_err();
        assert_eq!(err, TransmissionError::AuthFailed);

        // Basic auth was actually sent.
        let requests = server.await.unwrap();
        assert!(requests[0].to_lowercase().contains("authorization: basic"));
    }

    #[tokio::test]
    async fn test_connection_reads_version_and_rpc_errors_surface() {
        let (url, server) = mock_server(vec![
            (
                "200 OK",
                "",
                r#"{"result":"success","arguments":{"version":"4.0.6"}}"#,
            ),
            ("200 OK", "", r#"{"result":"method name not recognized"}"#),
        ])
        .await;

        let client = TransmissionClient::new(&url, "", "");
        assert_eq!(client.test_connection().await.unwrap(), "4.0.6");

        let err = client.pause_all().await.unwrap_err();
        assert_eq!(
            err,
            TransmissionError::RequestFailed("method name not recognized".to_string())
        );
        server.await.unwrap();
    }

    /// Read-only smoke test against a real Transmission instance (default
    /// http://localhost:9091; override with TRANSMISSION_TEST_URL).
    /// Run with: cargo test live_ -- --ignored --nocapture
    #[tokio::test]
    #[ignore = "requires a running Transmission instance"]
    async fn live_read_only_smoke_test() {
        let url = std::env::var("TRANSMISSION_TEST_URL")
            .unwrap_or_else(|_| "http://localhost:9091".to_string());
        let client = TransmissionClient::new(&url, "", "");

        let version = client.test_connection().await.unwrap();
        println!("connected: Transmission {version}");
        assert!(!version.is_empty());

        let limits = client.get_speed_limits().await.unwrap();
        println!("current limits (bytes/sec, 0 = unlimited): {limits:?}");
    }

    /// Full mutating round-trip against a real Transmission instance:
    /// throttles then restores the global speed limits, and pauses then
    /// resumes all torrents, re-stopping any that were already stopped so the
    /// instance is left exactly as found.
    /// Run with: cargo test live_mutating -- --ignored --nocapture
    #[tokio::test]
    #[ignore = "mutates a running Transmission instance"]
    async fn live_mutating_round_trip() {
        async fn statuses(client: &TransmissionClient) -> Vec<(u64, u64)> {
            let args = client
                .send("torrent-get", json!({ "fields": ["id", "status"] }))
                .await
                .unwrap();
            args.get("torrents")
                .and_then(Value::as_array)
                .unwrap()
                .iter()
                .map(|t| {
                    (
                        t.get("id").and_then(Value::as_u64).unwrap(),
                        t.get("status").and_then(Value::as_u64).unwrap(),
                    )
                })
                .collect()
        }

        let url = std::env::var("TRANSMISSION_TEST_URL")
            .unwrap_or_else(|_| "http://localhost:9091".to_string());
        let client = TransmissionClient::new(&url, "", "");

        // --- Speed limit round-trip, mirroring the engine's throttle flow.
        let saved = client.get_speed_limits().await.unwrap();
        println!("saved limits: {saved:?}");

        // The engine converts configured KB/s to bytes with * 1024.
        let throttle = SpeedLimits {
            download: 100 * 1024,
            upload: 50 * 1024,
        };
        client.set_speed_limits(&throttle).await.unwrap();
        let applied = client.get_speed_limits().await.unwrap();
        println!("throttled limits: {applied:?}");
        // 102400 bytes -> 102 KBps -> 102000 bytes; 51200 -> 51 -> 51000.
        assert_eq!(
            applied,
            SpeedLimits {
                download: 102_000,
                upload: 51_000,
            }
        );

        client.set_speed_limits(&saved).await.unwrap();
        let restored = client.get_speed_limits().await.unwrap();
        println!("restored limits: {restored:?}");
        assert_eq!(restored, saved);

        // --- Pause/resume round-trip. Status 0 = stopped.
        let before = statuses(&client).await;
        println!("torrents before: {before:?}");

        client.pause_all().await.unwrap();
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        let paused = statuses(&client).await;
        println!("after pause_all: {paused:?}");
        assert!(paused.iter().all(|(_, status)| *status == 0));

        client.resume_all().await.unwrap();
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;

        // Re-stop torrents that were already stopped before the test.
        let originally_stopped: Vec<u64> = before
            .iter()
            .filter(|(_, status)| *status == 0)
            .map(|(id, _)| *id)
            .collect();
        if !originally_stopped.is_empty() {
            client
                .send("torrent-stop", json!({ "ids": originally_stopped.clone() }))
                .await
                .unwrap();
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }

        let after = statuses(&client).await;
        println!("after resume + re-stop: {after:?}");
        assert_eq!(after.len(), before.len());
        for (id, status) in &after {
            if originally_stopped.contains(id) {
                assert_eq!(*status, 0, "torrent {id} should have been re-stopped");
            }
        }
    }
}
