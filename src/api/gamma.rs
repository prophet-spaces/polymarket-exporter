use std::sync::Arc;

use anyhow::{Context, Result};
use serde::Deserialize;
use tracing::debug;

use crate::rate_limiter::RateLimiterService;

const GAMMA_BASE: &str = "https://gamma-api.polymarket.com";

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GammaEvent {
    #[allow(dead_code)]
    pub id: String,
    #[allow(dead_code)]
    pub slug: Option<String>,
    pub title: Option<String>,
    pub markets: Option<Vec<GammaMarket>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct GammaMarket {
    pub id: String,
    pub question: Option<String>,
    pub condition_id: String,
    pub slug: Option<String>,
    pub outcomes: Option<String>,
    pub clob_token_ids: Option<String>,
    pub active: Option<bool>,
    pub closed: Option<bool>,
    pub order_price_min_tick_size: Option<f64>,
    pub order_min_size: Option<f64>,
}

/// Parsed token info extracted from a GammaMarket.
#[derive(Debug, Clone)]
pub struct TokenInfo {
    pub token_id: String,
    pub outcome: String,
}

impl GammaMarket {
    /// Parse `clobTokenIds` (JSON array string) and `outcomes` (JSON array string)
    /// into a vec of TokenInfo.
    pub fn token_infos(&self) -> Vec<TokenInfo> {
        let token_ids: Vec<String> = self
            .clob_token_ids
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or_default();

        let outcomes: Vec<String> = self
            .outcomes
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or_default();

        token_ids
            .into_iter()
            .zip(outcomes.into_iter().chain(std::iter::repeat_with(|| "Unknown".to_string())))
            .map(|(token_id, outcome)| TokenInfo { token_id, outcome })
            .collect()
    }
}

/// Unified resolution result that works for both market and event slugs.
pub struct ResolvedSlug {
    pub title: String,
    pub markets: Vec<GammaMarket>,
}

pub struct GammaClient {
    http: reqwest::Client,
    rate_limiter: Arc<RateLimiterService>,
}

impl GammaClient {
    pub fn new(http: reqwest::Client, rate_limiter: Arc<RateLimiterService>) -> Self {
        Self { http, rate_limiter }
    }

    /// Resolve a slug to markets. Tries the market endpoint first (since
    /// Polymarket URLs use market slugs), then falls back to the event endpoint.
    pub async fn resolve_slug(&self, slug: &str) -> Result<ResolvedSlug> {
        if let Some(resolved) = self.try_market_slug(slug).await? {
            return Ok(resolved);
        }

        debug!("slug '{}' not found as market, trying event endpoint", slug);
        self.get_event_by_slug(slug).await
    }

    async fn try_market_slug(&self, slug: &str) -> Result<Option<ResolvedSlug>> {
        self.rate_limiter.acquire("gamma", "events").await;

        let url = format!("{}/markets", GAMMA_BASE);
        let resp = self
            .http
            .get(&url)
            .query(&[("slug", slug)])
            .send()
            .await
            .context("failed to request Gamma market by slug")?;

        if !resp.status().is_success() {
            return Ok(None);
        }

        let markets: Vec<GammaMarket> = resp.json().await.unwrap_or_default();
        if markets.is_empty() {
            return Ok(None);
        }

        let title = markets
            .first()
            .and_then(|m| m.question.clone())
            .unwrap_or_default();

        debug!(
            "resolved slug '{}' via market endpoint ({} market(s))",
            slug,
            markets.len()
        );

        Ok(Some(ResolvedSlug { title, markets }))
    }

    async fn get_event_by_slug(&self, slug: &str) -> Result<ResolvedSlug> {
        self.rate_limiter.acquire("gamma", "events").await;

        let url = format!("{}/events/slug/{}", GAMMA_BASE, slug);
        let resp = self
            .http
            .get(&url)
            .send()
            .await
            .context("failed to request Gamma event by slug")?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!(
                "slug '{}' not found on either /markets or /events endpoint: {}",
                slug,
                body
            );
        }

        let event: GammaEvent = resp
            .json()
            .await
            .context("failed to deserialize Gamma event")?;

        let title = event.title.unwrap_or_default();
        let markets = event.markets.unwrap_or_default();

        debug!(
            "resolved slug '{}' via event endpoint ({} market(s))",
            slug,
            markets.len()
        );

        Ok(ResolvedSlug { title, markets })
    }
}

#[cfg(test)]
#[path = "../tests/gamma.rs"]
mod tests;
