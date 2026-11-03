use std::sync::Arc;

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::rate_limiter::RateLimiterService;

const DATA_API_BASE: &str = "https://data-api.polymarket.com";

#[derive(Debug, Clone, Deserialize)]
pub struct OpenInterest {
    #[serde(rename = "condition_id")]
    pub market: String,
    pub value: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MetaHolder {
    #[serde(rename = "token_id")]
    pub token: Option<String>,
    pub holders: Option<Vec<Holder>>,
}

/// An open position returned by the Data API for a wallet.
#[derive(Debug, Clone, Deserialize)]
pub struct Position {
    #[serde(rename = "token_id")]
    pub asset: String,
    pub condition_id: String,
    #[serde(rename = "current_size")]
    pub size: f64,
    pub avg_price: f64,
    #[serde(rename = "entry_cost_usdc")]
    pub initial_value: f64,
    pub current_value: f64,
    #[serde(rename = "unrealized_pnl")]
    pub cash_pnl: f64,
    pub percent_pnl: f64,
    pub realized_pnl: f64,
    #[serde(rename = "current_price")]
    pub cur_price: f64,
    pub redeemable: bool,
    pub title: String,
    pub outcome: String,
}

#[derive(Debug, Clone, Deserialize)]
struct DataResponse<T> {
    data: T,
}

#[derive(Debug, Deserialize)]
struct PositionsResponse {
    data: Vec<Position>,
    pagination: Pagination,
}

#[derive(Debug, Deserialize)]
struct Pagination {
    has_more: bool,
    next_cursor: Option<String>,
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
    base_url: String,
}

impl DataClient {
    pub fn new(http: reqwest::Client, rate_limiter: Arc<RateLimiterService>) -> Self {
        Self {
            http,
            rate_limiter,
            base_url: DATA_API_BASE.to_string(),
        }
    }

    /// Fetch open interest for one or more condition IDs.
    pub async fn get_open_interest(&self, condition_ids: &[&str]) -> Result<Vec<OpenInterest>> {
        let mut result = Vec::new();
        for batch in condition_ids.chunks(20) {
            self.rate_limiter.acquire("data_api", "general").await;
            let resp = self
                .http
                .get(format!("{}/v2/oi", self.base_url))
                .query(&[("condition", batch.join(","))])
                .send()
                .await
                .context("failed to request open interest")?;

            let status = resp.status();
            if !status.is_success() {
                let body = resp.text().await.unwrap_or_default();
                anyhow::bail!("Data API /v2/oi returned {}: {}", status, body);
            }

            let response: DataResponse<Vec<OpenInterest>> = resp
                .json()
                .await
                .context("failed to deserialize open interest")?;
            result.extend(response.data);
        }
        Ok(result)
    }

    /// Fetch top holders for one or more condition IDs (max 20 per token).
    pub async fn get_top_holders(&self, condition_ids: &[&str]) -> Result<Vec<MetaHolder>> {
        let mut result = Vec::new();
        for batch in condition_ids.chunks(20) {
            self.rate_limiter.acquire("data_api", "holders").await;
            self.rate_limiter.acquire("data_api", "general").await;
            let condition = batch.join(",");
            let resp = self
                .http
                .get(format!("{}/v2/holders", self.base_url))
                .query(&[("condition", condition.as_str()), ("limit", "20")])
                .send()
                .await
                .context("failed to request top holders")?;

            let status = resp.status();
            if !status.is_success() {
                let body = resp.text().await.unwrap_or_default();
                anyhow::bail!("Data API /v2/holders returned {}: {}", status, body);
            }

            // Only the first page is needed for the top 20 holders per token.
            let response: DataResponse<Vec<MetaHolder>> = resp
                .json()
                .await
                .context("failed to deserialize top holders")?;
            result.extend(response.data);
        }
        Ok(result)
    }

    /// Fetch the currently open positions for a wallet.
    pub async fn get_positions(&self, wallet: &str) -> Result<Vec<Position>> {
        let mut positions = Vec::new();
        let mut cursor = None;
        let mut seen_cursors = std::collections::HashSet::new();
        loop {
            self.rate_limiter.acquire("data_api", "positions").await;
            self.rate_limiter.acquire("data_api", "general").await;
            let mut request = self
                .http
                .get(format!("{}/v2/positions", self.base_url))
                .query(&[
                    ("user", wallet),
                    ("status", "OPEN"),
                    ("filter_type", "TOKENS"),
                    ("filter_amount", "0"),
                    ("limit", "1000"),
                ]);
            if let Some(cursor) = &cursor {
                request = request.query(&[("cursor", cursor)]);
            }
            let resp = request
                .send()
                .await
                .context("failed to request wallet positions")?;

            let status = resp.status();
            if !status.is_success() {
                let body = resp.text().await.unwrap_or_default();
                anyhow::bail!("Data API /v2/positions returned {}: {}", status, body);
            }

            let response: PositionsResponse = resp
                .json()
                .await
                .context("failed to deserialize wallet positions")?;
            anyhow::ensure!(
                response.pagination.has_more == response.pagination.next_cursor.is_some(),
                "Data API /v2/positions returned inconsistent pagination"
            );
            positions.extend(response.data);
            cursor = response.pagination.next_cursor;
            match &cursor {
                None => return Ok(positions),
                Some(next) => anyhow::ensure!(
                    seen_cursors.insert(next.clone()),
                    "Data API /v2/positions returned a repeated cursor"
                ),
            }
        }
    }

    /// Fetch the all-time realized P&L reported for a wallet.
    pub async fn get_all_time_realized_pnl(&self, wallet: &str) -> Result<Option<f64>> {
        self.rate_limiter.acquire("data_api", "general").await;

        let url = format!("{}/v2/user-stats", self.base_url);
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

        let stats: DataResponse<Option<UserStats>> = resp
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
