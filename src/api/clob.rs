use std::sync::Arc;

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::rate_limiter::RateLimiterService;

const CLOB_BASE: &str = "https://clob.polymarket.com";

#[derive(Debug, Clone, Deserialize)]
pub struct OrderBookSummary {
    #[allow(dead_code)]
    pub market: String,
    #[allow(dead_code)]
    pub asset_id: String,
    pub bids: Vec<OrderLevel>,
    pub asks: Vec<OrderLevel>,
    pub min_order_size: Option<String>,
    pub tick_size: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OrderLevel {
    pub price: String,
    #[allow(dead_code)]
    pub size: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ClobMarket {
    pub tokens: Vec<ClobToken>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ClobToken {
    pub token_id: String,
    pub outcome: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FeeRateResponse {
    pub base_fee: i64,
}

#[derive(Debug, Deserialize)]
struct LastTradePriceResponse {
    price: String,
    side: String,
}

impl LastTradePriceResponse {
    fn into_price(self) -> Result<Option<f64>> {
        match self.side.as_str() {
            // The Polymarket API returns a placeholder price of 0.5 when there are no trades.
            "" => return Ok(None),
            "BUY" | "SELL" => {}
            _ => anyhow::bail!("invalid last trade side: {}", self.side),
        }
        let price: f64 = self.price.parse().context("invalid last trade price")?;
        anyhow::ensure!(
            price.is_finite() && (0.0..=1.0).contains(&price),
            "last trade price out of range: {}",
            price
        );
        Ok(Some(price))
    }
}

pub struct ClobClient {
    http: reqwest::Client,
    rate_limiter: Arc<RateLimiterService>,
    base_url: String,
}

impl ClobClient {
    pub fn new(http: reqwest::Client, rate_limiter: Arc<RateLimiterService>) -> Self {
        Self {
            http,
            rate_limiter,
            base_url: CLOB_BASE.to_string(),
        }
    }

    pub async fn get_book(&self, token_id: &str) -> Result<OrderBookSummary> {
        self.rate_limiter.acquire("clob", "book").await;

        let url = format!("{}/book", self.base_url);
        let resp = self
            .http
            .get(&url)
            .query(&[("token_id", token_id)])
            .send()
            .await
            .context("failed to request CLOB orderbook")?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("CLOB /book returned {}: {}", status, body);
        }

        resp.json().await.context("failed to deserialize orderbook")
    }

    pub async fn get_last_trade_price(&self, token_id: &str) -> Result<Option<f64>> {
        self.rate_limiter.acquire("clob", "last_trade_price").await;

        let url = format!("{}/last-trade-price", self.base_url);
        let resp = self
            .http
            .get(&url)
            .query(&[("token_id", token_id)])
            .send()
            .await
            .context("failed to request CLOB last trade price")?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("CLOB /last-trade-price returned {}: {}", status, body);
        }

        let trade: LastTradePriceResponse = resp
            .json()
            .await
            .context("failed to deserialize last trade price")?;
        trade.into_price()
    }

    pub async fn get_fee_rate(&self, token_id: &str) -> Result<FeeRateResponse> {
        self.rate_limiter.acquire("clob", "fee_rate").await;

        let url = format!("{}/fee-rate", self.base_url);
        let resp = self
            .http
            .get(&url)
            .query(&[("token_id", token_id)])
            .send()
            .await
            .context("failed to request CLOB fee rate")?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("CLOB /fee-rate returned {}: {}", status, body);
        }

        resp.json().await.context("failed to deserialize fee rate")
    }

    pub async fn get_market(&self, condition_id: &str) -> Result<ClobMarket> {
        self.rate_limiter.acquire("clob", "markets").await;

        let url = format!("{}/markets/{}", self.base_url, condition_id);
        let resp = self
            .http
            .get(&url)
            .send()
            .await
            .context("failed to request CLOB market")?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("CLOB /markets returned {}: {}", status, body);
        }

        resp.json()
            .await
            .context("failed to deserialize CLOB market")
    }
}

#[cfg(test)]
#[path = "../tests/clob.rs"]
mod tests;
