//! The HTTP transport for the SSE adapter: turning a URL and a dialect into a byte stream,
//! and the reconnect backoff. What frames mean belongs to [`super::dialect`], not here.

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use tokio_stream::StreamExt;

use super::ByteStream;
use super::dialect::SseDialect;

pub(super) trait Connect: Send + Sync + 'static {
    fn connect<'a>(
        &'a self,
        since: Option<&'a str>,
        last_event_id: Option<&'a str>,
    ) -> impl Future<Output = Result<ByteStream, ConnectError>> + Send + 'a;
}

#[derive(Debug, thiserror::Error)]
pub(super) enum ConnectError {
    #[error("HTTP transport failed: {0}")]
    Transport(String),
    #[error("HTTP response had status {0}")]
    HttpStatus(u16),
    #[error("HTTP request could not be built: {0}")]
    Request(String),
}

pub(super) struct ReqwestConnect {
    client: reqwest::Client,
    url: reqwest::Url,
    user_agent: &'static str,
    dialect: Arc<dyn SseDialect>,
}

impl ReqwestConnect {
    pub(super) fn new(
        url: reqwest::Url,
        user_agent: &'static str,
        dialect: Arc<dyn SseDialect>,
    ) -> Result<Self, String> {
        let client = reqwest::Client::builder()
            .build()
            .map_err(|error| format!("could not initialize the HTTP client: {error}"))?;
        Ok(Self {
            client,
            url,
            user_agent,
            dialect,
        })
    }

    pub(super) fn build_request(
        &self,
        since: Option<&str>,
        last_event_id: Option<&str>,
    ) -> Result<reqwest::Request, ConnectError> {
        let mut url = self.url.clone();
        if let Some(value) = since {
            self.dialect
                .apply_since(&mut url, value)
                .map_err(|error| ConnectError::Request(error.to_string()))?;
        }
        let mut request = self.client.get(url).header("User-Agent", self.user_agent);
        if let Some(value) = last_event_id {
            request = request.header("Last-Event-ID", value);
        }
        request
            .build()
            .map_err(|error| ConnectError::Request(error.to_string()))
    }
}

impl Connect for ReqwestConnect {
    async fn connect<'a>(
        &'a self,
        since: Option<&'a str>,
        last_event_id: Option<&'a str>,
    ) -> Result<ByteStream, ConnectError> {
        let request = self.build_request(since, last_event_id)?;
        let response = self
            .client
            .execute(request)
            .await
            .map_err(|error| ConnectError::Transport(error.to_string()))?;
        if !response.status().is_success() {
            return Err(ConnectError::HttpStatus(response.status().as_u16()));
        }
        let stream = response.bytes_stream().map(|chunk| {
            chunk
                .map(|bytes| bytes.to_vec())
                .map_err(|error| ConnectError::Transport(error.to_string()))
        });
        Ok(Box::pin(stream) as ByteStream)
    }
}

#[derive(Clone, Copy)]
pub(super) struct Backoff {
    initial: Duration,
    maximum: Duration,
    current: Duration,
}

impl Backoff {
    pub(super) const fn production() -> Self {
        Self {
            initial: Duration::from_millis(250),
            maximum: Duration::from_secs(30),
            current: Duration::from_millis(250),
        }
    }

    #[cfg(test)]
    pub(super) const fn fixed(duration: Duration) -> Self {
        Self {
            initial: duration,
            maximum: duration,
            current: duration,
        }
    }

    pub(super) async fn wait(&mut self) {
        tokio::time::sleep(self.current).await;
        self.current = self.current.saturating_mul(2).min(self.maximum);
    }

    pub(super) const fn current(&self) -> Duration {
        self.current
    }

    pub(super) fn reset(&mut self) {
        self.current = self.initial;
    }
}
