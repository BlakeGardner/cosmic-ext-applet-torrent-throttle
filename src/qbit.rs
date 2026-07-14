use reqwest::Client;
use std::sync::Arc;
use tokio::sync::Mutex;

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

    pub async fn pause_all(&self) -> Result<(), QbitError> {
        self.ensure_authenticated().await?;

        let url = format!("{}/api/v2/torrents/pause", self.base_url);
        let params = [("hashes", "all")];

        let response = self
            .client
            .post(&url)
            .form(&params)
            .send()
            .await
            .map_err(|e| QbitError::RequestFailed(e.to_string()))?;

        if response.status() == reqwest::StatusCode::FORBIDDEN {
            // Session expired, re-auth and retry
            let mut auth = self.authenticated.lock().await;
            *auth = false;
            drop(auth);
            self.login().await?;

            self.client
                .post(&url)
                .form(&params)
                .send()
                .await
                .map_err(|e| QbitError::RequestFailed(e.to_string()))?;
        }

        Ok(())
    }

    pub async fn resume_all(&self) -> Result<(), QbitError> {
        self.ensure_authenticated().await?;

        let url = format!("{}/api/v2/torrents/resume", self.base_url);
        let params = [("hashes", "all")];

        let response = self
            .client
            .post(&url)
            .form(&params)
            .send()
            .await
            .map_err(|e| QbitError::RequestFailed(e.to_string()))?;

        if response.status() == reqwest::StatusCode::FORBIDDEN {
            let mut auth = self.authenticated.lock().await;
            *auth = false;
            drop(auth);
            self.login().await?;

            self.client
                .post(&url)
                .form(&params)
                .send()
                .await
                .map_err(|e| QbitError::RequestFailed(e.to_string()))?;
        }

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
