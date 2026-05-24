use std::collections::HashMap;
use std::net::IpAddr;
use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

use governor::{
    Quota, RateLimiter, clock::DefaultClock, middleware::NoOpMiddleware, state::InMemoryState,
    state::direct::NotKeyed,
};

const USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

#[derive(Debug, thiserror::Error)]
pub enum SafeClientError {
    #[error("Invalid URL: {0}")]
    InvalidUrl(String),
    #[error("SSRF blocked: {0}")]
    SsrfBlocked(String),
    #[error("Robots.txt disallowed: {0}")]
    RobotsDisallowed(String),
    #[error("HTTP error: {status}")]
    Http { status: u16 },
    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),
    #[error("DNS resolution failed: {0}")]
    Dns(String),
    #[error("Request timed out after {0}s")]
    Timeout(u64),
}

pub struct SafeClient {
    client: reqwest::Client,
    limiter: Arc<RateLimiter<NotKeyed, InMemoryState, DefaultClock, NoOpMiddleware>>,
    robots_cache: Arc<Mutex<HashMap<String, texting_robots::Robot>>>,
    timeout: Duration,
}

impl SafeClient {
    pub fn new(timeout_secs: u64) -> Self {
        let quota = Quota::per_second(NonZeroU32::new(5).unwrap());
        let limiter = Arc::new(RateLimiter::direct(quota));
        Self {
            client: reqwest::Client::builder()
                .user_agent(USER_AGENT)
                .timeout(Duration::from_secs(timeout_secs))
                .build()
                .expect("reqwest client build"),
            limiter,
            robots_cache: Arc::new(Mutex::new(HashMap::new())),
            timeout: Duration::from_secs(timeout_secs),
        }
    }

    pub async fn fetch(
        &self,
        url_str: &str,
        ignore_robots: bool,
    ) -> Result<String, SafeClientError> {
        let parsed =
            url::Url::parse(url_str).map_err(|e| SafeClientError::InvalidUrl(e.to_string()))?;

        self.check_ssrf(&parsed).await?;

        if !ignore_robots {
            self.check_robots(&parsed).await?;
        }

        self.limiter.until_ready().await;

        let response = tokio::time::timeout(self.timeout, self.client.get(url_str).send())
            .await
            .map_err(|_| SafeClientError::Timeout(self.timeout.as_secs()))??;

        let status = response.status();
        if !status.is_success() {
            return Err(SafeClientError::Http {
                status: status.as_u16(),
            });
        }

        Ok(response.text().await?)
    }

    async fn check_ssrf(&self, parsed: &url::Url) -> Result<(), SafeClientError> {
        let host = parsed
            .host_str()
            .ok_or_else(|| SafeClientError::InvalidUrl("no host".into()))?;

        if let Some(ip) = host.parse::<IpAddr>().ok() {
            if is_private_ip(&ip) {
                return Err(SafeClientError::SsrfBlocked(host.into()));
            }
            return Ok(());
        }

        let addrs = tokio::net::lookup_host((host, 0))
            .await
            .map_err(|e| SafeClientError::Dns(e.to_string()))?;

        for addr in addrs {
            if is_private_ip(&addr.ip()) {
                return Err(SafeClientError::SsrfBlocked(format!(
                    "{host} resolves to {addr}"
                )));
            }
        }

        Ok(())
    }

    async fn check_robots(&self, parsed: &url::Url) -> Result<(), SafeClientError> {
        let domain = parsed.host_str().unwrap_or("").to_string();
        let url_str = parsed.as_str();

        {
            let cache = self.robots_cache.lock().await;
            if let Some(robot) = cache.get(&domain) {
                if !robot.allowed(url_str) {
                    return Err(SafeClientError::RobotsDisallowed(parsed.to_string()));
                }
                return Ok(());
            }
        }

        let robots_url = format!("{}://{}/robots.txt", parsed.scheme(), domain);
        let body =
            match tokio::time::timeout(self.timeout, self.client.get(&robots_url).send()).await {
                Ok(Ok(resp)) => match resp.text().await {
                    Ok(t) => t,
                    Err(_) => String::new(),
                },
                _ => String::new(),
            };

        if let Ok(robot) = texting_robots::Robot::new(USER_AGENT, body.as_bytes()) {
            let allowed = robot.allowed(url_str);
            let mut cache = self.robots_cache.lock().await;
            cache.insert(domain, robot);
            if !allowed {
                return Err(SafeClientError::RobotsDisallowed(parsed.to_string()));
            }
        }

        Ok(())
    }
}

fn is_private_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.octets()[0] == 0
        }
        IpAddr::V6(v6) => {
            v6.is_loopback() || v6.is_unspecified() || (v6.segments()[0] & 0xfe00) == 0xfc00
        }
    }
}
