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

/// An open position returned by the Data API for a wallet.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Position {
    pub asset: String,
    pub condition_id: String,
    pub size: f64,
    pub avg_price: f64,
    pub initial_value: f64,
    pub current_value: f64,
    pub cash_pnl: f64,
    pub percent_pnl: f64,
    pub realized_pnl: f64,
    pub cur_price: f64,
    pub redeemable: bool,
    pub title: String,
    pub outcome: String,
}

#[derive(Debug, Clone, Deserialize)]
struct UserStatsResponse {
    data: Option<UserStats>,
}

#[derive(Debug, Clone, Deserialize)]
struct UserStats {
    all_time_pnl: Option<AllTimePnl>,
}

#[derive(Debug, Clone, Deserialize)]
struct AllTimePnl {
    realized_pnl: Option<f64>,
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

        resp.json()
            .await
            .context("failed to deserialize open interest")
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

        resp.json()
            .await
            .context("failed to deserialize top holders")
    }

    /// Fetch the currently open positions for a wallet.
    pub async fn get_positions(&self, wallet: &str) -> Result<Vec<Position>> {
        self.rate_limiter.acquire("data_api", "general").await;

        let url = format!("{}/positions", DATA_API_BASE);
        let resp = self
            .http
            .get(&url)
            .query(&[("user", wallet), ("sizeThreshold", "0")])
            .send()
            .await
            .context("failed to request wallet positions")?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Data API /positions returned {}: {}", status, body);
        }

        resp.json()
            .await
            .context("failed to deserialize wallet positions")
    }

    /// Fetch the all-time realized P&L reported for a wallet.
    pub async fn get_all_time_realized_pnl(&self, wallet: &str) -> Result<Option<f64>> {
        self.rate_limiter.acquire("data_api", "general").await;

        let url = format!("{}/v2/user-stats", DATA_API_BASE);
        let resp = self
            .http
            .get(&url)
            .query(&[("user", wallet)])
            .send()
            .await
            .context("failed to request wallet stats")?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Data API /v2/user-stats returned {}: {}", status, body);
        }

        let stats: UserStatsResponse = resp
            .json()
            .await
            .context("failed to deserialize wallet stats")?;

        Ok(stats
            .data
            .and_then(|stats| stats.all_time_pnl)
            .and_then(|pnl| pnl.realized_pnl))
    }
}

#[cfg(test)]
#[path = "../tests/data.rs"]
mod tests;
