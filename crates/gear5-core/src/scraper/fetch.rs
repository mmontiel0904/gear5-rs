use crate::config::ScrapeConfig;
use crate::Result;
use rand::Rng;
use reqwest::{Client, Response, StatusCode, Url};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;
use tokio::time::sleep;

const RETRY_ATTEMPTS: u32 = 3;
const RETRY_BASE_MS: u64 = 400;

#[derive(Clone)]
pub struct HttpClient {
    inner: Client,
    base_url: Url,
    limiter: Arc<Semaphore>,
    jitter_min: u64,
    jitter_max: u64,
}

impl HttpClient {
    pub fn new(cfg: &ScrapeConfig) -> Result<Self> {
        let inner = Client::builder()
            .user_agent(&cfg.user_agent)
            .timeout(Duration::from_secs(15))
            .gzip(true)
            .brotli(true)
            .pool_idle_timeout(Some(Duration::from_secs(30)))
            .build()?;
        let base_url = Url::parse(&cfg.base_url)?;
        let limiter = Arc::new(Semaphore::new(cfg.concurrency.max(1)));
        let (jitter_min, jitter_max) = if cfg.jitter_ms_max >= cfg.jitter_ms_min {
            (cfg.jitter_ms_min, cfg.jitter_ms_max)
        } else {
            (cfg.jitter_ms_min, cfg.jitter_ms_min + 1)
        };
        Ok(Self {
            inner,
            base_url,
            limiter,
            jitter_min,
            jitter_max,
        })
    }

    pub fn base_url(&self) -> &Url {
        &self.base_url
    }

    pub async fn fetch_index(&self) -> Result<String> {
        let _p = self.limiter.acquire().await.unwrap();
        self.polite_delay().await;
        let resp = self
            .send_with_retry(|| self.inner.get(self.base_url.clone()))
            .await?;
        let text = resp.text().await?;
        Ok(text)
    }

    /// The card list site filters by set via a POST form (`<form method="post">`); GET query
    /// strings are silently ignored and the server returns the default page instead. Issue a
    /// form-encoded POST so the requested series actually applies.
    pub async fn fetch_series(&self, series: &str) -> Result<String> {
        let _p = self.limiter.acquire().await.unwrap();
        self.polite_delay().await;
        let resp = self
            .send_with_retry(|| {
                self.inner
                    .post(self.base_url.clone())
                    .form(&[("series", series), ("freewords", "")])
            })
            .await?;
        let text = resp.text().await?;
        Ok(text)
    }

    /// Download an image (relative or absolute URL) and return the bytes.
    pub async fn fetch_image(&self, raw_url: &str) -> Result<bytes::Bytes> {
        let _p = self.limiter.acquire().await.unwrap();
        self.polite_delay().await;
        let url = self.base_url.join(raw_url)?;
        let resp = self.send_with_retry(|| self.inner.get(url.clone())).await?;
        let bytes = resp.bytes().await?;
        Ok(bytes)
    }

    async fn send_with_retry<F>(&self, build: F) -> Result<Response>
    where
        F: Fn() -> reqwest::RequestBuilder,
    {
        let mut last_err: Option<reqwest::Error> = None;
        for attempt in 0..RETRY_ATTEMPTS {
            match build().send().await {
                Ok(resp) => match resp.error_for_status_ref() {
                    Ok(_) => return Ok(resp),
                    Err(e) => {
                        let status = resp.status();
                        if is_retryable_status(status) && attempt + 1 < RETRY_ATTEMPTS {
                            tracing::warn!(status = %status, attempt, "retrying after http error");
                            last_err = Some(e);
                            sleep(retry_backoff(attempt)).await;
                            continue;
                        }
                        return Err(e.into());
                    }
                },
                Err(e) => {
                    if is_retryable_transport(&e) && attempt + 1 < RETRY_ATTEMPTS {
                        tracing::warn!(error = %e, attempt, "retrying after transport error");
                        last_err = Some(e);
                        sleep(retry_backoff(attempt)).await;
                        continue;
                    }
                    return Err(e.into());
                }
            }
        }
        Err(last_err
            .expect("retry loop exited without a stored error")
            .into())
    }

    async fn polite_delay(&self) {
        let ms = if self.jitter_max > self.jitter_min {
            rand::thread_rng().gen_range(self.jitter_min..self.jitter_max)
        } else {
            self.jitter_min
        };
        sleep(Duration::from_millis(ms)).await;
    }
}

fn is_retryable_status(status: StatusCode) -> bool {
    status.is_server_error() || status == StatusCode::TOO_MANY_REQUESTS
}

fn is_retryable_transport(err: &reqwest::Error) -> bool {
    err.is_timeout() || err.is_connect() || err.is_request()
}

fn retry_backoff(attempt: u32) -> Duration {
    Duration::from_millis(RETRY_BASE_MS << attempt)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    /// Spawn a minimal one-shot HTTP server that captures the first request and replies 200.
    async fn one_shot_server() -> (u16, tokio::task::JoinHandle<CapturedRequest>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = Vec::with_capacity(4096);
            // Read headers
            let mut tmp = [0u8; 1024];
            loop {
                let n = socket.read(&mut tmp).await.unwrap();
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&tmp[..n]);
                if buf.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            let header_end = buf.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
            let header_text = String::from_utf8_lossy(&buf[..header_end]).to_string();
            let mut content_length = 0usize;
            let mut method = String::new();
            let mut path = String::new();
            for (i, line) in header_text.split("\r\n").enumerate() {
                if i == 0 {
                    let mut parts = line.split_whitespace();
                    method = parts.next().unwrap_or("").to_string();
                    path = parts.next().unwrap_or("").to_string();
                } else if let Some(rest) = line
                    .to_ascii_lowercase()
                    .strip_prefix("content-length:")
                {
                    content_length = rest.trim().parse().unwrap_or(0);
                }
            }
            let body_start = header_end + 4;
            let mut body = buf[body_start..].to_vec();
            while body.len() < content_length {
                let n = socket.read(&mut tmp).await.unwrap();
                if n == 0 {
                    break;
                }
                body.extend_from_slice(&tmp[..n]);
            }
            body.truncate(content_length);
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nContent-Type: text/html\r\nConnection: close\r\n\r\nok")
                .await
                .unwrap();
            socket.shutdown().await.ok();
            CapturedRequest {
                method,
                path,
                body: String::from_utf8_lossy(&body).to_string(),
                headers: header_text,
            }
        });
        (port, handle)
    }

    struct CapturedRequest {
        method: String,
        path: String,
        body: String,
        headers: String,
    }

    fn test_client(port: u16) -> HttpClient {
        let cfg = ScrapeConfig {
            base_url: format!("http://127.0.0.1:{port}/cardlist/"),
            jitter_ms_min: 0,
            jitter_ms_max: 0,
            ..ScrapeConfig::default()
        };
        HttpClient::new(&cfg).expect("http client")
    }

    /// Regression: the site filters by set via POST, not GET. Issuing GET caused the scraper to
    /// receive the default page on every iteration, landing only one set's cards in the DB.
    #[tokio::test]
    async fn fetch_series_posts_form_encoded_series() {
        let (port, server) = one_shot_server().await;
        let http = test_client(port);
        let _ = http.fetch_series("569101").await.expect("fetch_series");
        let req = server.await.unwrap();
        assert_eq!(req.method, "POST", "headers:\n{}", req.headers);
        assert_eq!(req.path, "/cardlist/");
        assert!(
            req.headers
                .to_ascii_lowercase()
                .contains("content-type: application/x-www-form-urlencoded"),
            "expected form-encoded content type, got headers:\n{}",
            req.headers
        );
        assert!(
            req.body.contains("series=569101"),
            "body must contain series=569101, got: {}",
            req.body
        );
    }
}
