use std::sync::Arc;

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::rate_limiter::RateLimiterService;

const DATA_API_BASE: &str = "https://data-api.polymarket.com";

#[derive(Debug, Clone, Deserialize)]
pub struct OpenInterest {
    pub market: String,
    pub value: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MetaHolder {
    pub token: Option<String>,
    pub holders: Option<Vec<Holder>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Holder {
    pub proxy_wallet: Option<String>,
    pub name: Option<String>,
    pub pseudonym: Option<String>,
    pub amount: Option<f64>,
    #[allow(dead_code)]
    pub outcome_index: Option<i64>,
    #[allow(dead_code)]
    pub profile_image: Option<String>,
}

impl Holder {
    pub fn display_name(&self) -> String {
        self.name
            .as_deref()
            .filter(|n| !n.is_empty())
            .or(self.pseudonym.as_deref().filter(|n| !n.is_empty()))
            .or(self.proxy_wallet.as_deref())
            .unwrap_or("unknown")
            .to_string()
    }
}

pub struct DataClient {
    http: reqwest::Client,
    rate_limiter: Arc<RateLimiterService>,
}

impl DataClient {
    pub fn new(http: reqwest::Client, rate_limiter: Arc<RateLimiterService>) -> Self {
        Self { http, rate_limiter }
    }

    /// Fetch open interest for one or more condition IDs.
    pub async fn get_open_interest(&self, condition_ids: &[&str]) -> Result<Vec<OpenInterest>> {
        self.rate_limiter.acquire("data_api", "general").await;

        let market_param = condition_ids.join(",");
        let url = format!("{}/oi", DATA_API_BASE);
        let resp = self
            .http
            .get(&url)
            .query(&[("market", &market_param)])
            .send()
            .await
            .context("failed to request open interest")?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Data API /oi returned {}: {}", status, body);
        }

        resp.json().await.context("failed to deserialize open interest")
    }

    /// Fetch top holders for one or more condition IDs (max 20 per token).
    pub async fn get_top_holders(&self, condition_ids: &[&str]) -> Result<Vec<MetaHolder>> {
        self.rate_limiter.acquire("data_api", "holders").await;

        let market_param = condition_ids.join(",");
        let url = format!("{}/holders", DATA_API_BASE);
        let resp = self
            .http
            .get(&url)
            .query(&[("market", &market_param), ("limit", &"20".to_string())])
            .send()
            .await
            .context("failed to request top holders")?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Data API /holders returned {}: {}", status, body);
        }

        resp.json().await.context("failed to deserialize top holders")
    }
}

#[cfg(test)]
#[path = "../tests/data.rs"]
mod tests;
