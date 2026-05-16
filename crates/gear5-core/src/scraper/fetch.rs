use crate::config::ScrapeConfig;
use crate::Result;
use rand::Rng;
use reqwest::{Client, Url};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;
use tokio::time::sleep;

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
        let resp = self.inner.get(self.base_url.clone()).send().await?;
        let text = resp.error_for_status()?.text().await?;
        Ok(text)
    }

    pub async fn fetch_series(&self, series: &str) -> Result<String> {
        let _p = self.limiter.acquire().await.unwrap();
        self.polite_delay().await;
        let url = {
            let mut u = self.base_url.clone();
            u.query_pairs_mut().clear().append_pair("series", series);
            u
        };
        let resp = self.inner.get(url).send().await?;
        let text = resp.error_for_status()?.text().await?;
        Ok(text)
    }

    /// Download an image (relative or absolute URL) and return the bytes.
    pub async fn fetch_image(&self, raw_url: &str) -> Result<bytes::Bytes> {
        let _p = self.limiter.acquire().await.unwrap();
        self.polite_delay().await;
        let url = self.base_url.join(raw_url)?;
        let resp = self.inner.get(url).send().await?;
        let bytes = resp.error_for_status()?.bytes().await?;
        Ok(bytes)
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
