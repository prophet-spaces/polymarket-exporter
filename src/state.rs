use std::collections::HashMap;
use std::time::{Duration, Instant};

use tokio::sync::RwLock;

use crate::api::data::{Holder, MetaHolder, OpenInterest};
use crate::api::gamma::{ResolvedSlug, TokenInfo};
use crate::config::CacheConfig;

/// Per-token real-time data, updated by WebSocket events.
#[derive(Debug, Clone, Default)]
pub struct TokenState {
    pub best_bid: Option<f64>,
    pub best_ask: Option<f64>,
    pub spread: Option<f64>,
    pub last_trade_price: Option<f64>,
    pub tick_size: Option<f64>,
    pub min_order_size: Option<f64>,
    pub fee_rate_bps: Option<i64>,
}

/// Resolved market metadata (immutable after activation).
#[derive(Debug, Clone)]
pub struct MarketMeta {
    pub question: String,
    pub condition_id: String,
    pub tokens: Vec<TokenInfo>,
}

/// Cached REST data with TTL.
#[derive(Debug, Clone)]
struct CachedData<T> {
    data: T,
    fetched_at: Instant,
}

impl<T> CachedData<T> {
    fn is_expired(&self, ttl: Duration) -> bool {
        self.fetched_at.elapsed() > ttl
    }
}

/// All state for a single slug.
#[derive(Debug)]
pub struct SlugState {
    #[allow(dead_code)]
    pub slug: String,
    #[allow(dead_code)]
    pub event_title: String,
    pub markets: Vec<MarketMeta>,
    /// token_id -> TokenState
    pub tokens: HashMap<String, TokenState>,
    /// condition_id -> OpenInterest value
    open_interest: Option<CachedData<HashMap<String, f64>>>,
    /// token_id -> Vec<Holder>
    top_holders: Option<CachedData<HashMap<String, Vec<Holder>>>>,
}

impl SlugState {
    #[cfg(test)]
    pub fn new_for_test(
        slug: &str,
        markets: Vec<MarketMeta>,
        tokens: HashMap<String, TokenState>,
    ) -> Self {
        Self {
            slug: slug.to_string(),
            event_title: String::new(),
            markets,
            tokens,
            open_interest: None,
            top_holders: None,
        }
    }

    pub fn get_open_interest(&self, condition_id: &str) -> Option<f64> {
        self.open_interest
            .as_ref()
            .and_then(|c| c.data.get(condition_id).copied())
    }

    pub fn get_top_holders(&self, token_id: &str) -> Option<&Vec<Holder>> {
        self.top_holders
            .as_ref()
            .and_then(|c| c.data.get(token_id))
    }

    pub fn is_oi_expired(&self, ttl: Duration) -> bool {
        self.open_interest
            .as_ref()
            .map(|c| c.is_expired(ttl))
            .unwrap_or(true)
    }

    pub fn is_holders_expired(&self, ttl: Duration) -> bool {
        self.top_holders
            .as_ref()
            .map(|c| c.is_expired(ttl))
            .unwrap_or(true)
    }

    pub fn set_open_interest(&mut self, data: Vec<OpenInterest>) {
        let mut map = HashMap::new();
        for oi in data {
            if let Some(v) = oi.value {
                map.insert(oi.market, v);
            }
        }
        self.open_interest = Some(CachedData {
            data: map,
            fetched_at: Instant::now(),
        });
    }

    pub fn set_top_holders(&mut self, data: Vec<MetaHolder>) {
        let mut map: HashMap<String, Vec<Holder>> = HashMap::new();
        for mh in data {
            if let (Some(token), Some(holders)) = (mh.token, mh.holders) {
                map.insert(token, holders);
            }
        }
        self.top_holders = Some(CachedData {
            data: map,
            fetched_at: Instant::now(),
        });
    }
}

/// Global registry of all active slugs.
pub struct SlugRegistry {
    pub slugs: RwLock<HashMap<String, SlugState>>,
    /// Reverse lookup: token_id -> slug (for WebSocket event dispatch)
    pub token_to_slug: RwLock<HashMap<String, String>>,
    pub cache_config: CacheConfig,
}

impl SlugRegistry {
    pub fn new(cache_config: CacheConfig) -> Self {
        Self {
            slugs: RwLock::new(HashMap::new()),
            token_to_slug: RwLock::new(HashMap::new()),
            cache_config,
        }
    }

    pub async fn is_active(&self, slug: &str) -> bool {
        self.slugs.read().await.contains_key(slug)
    }

    /// Register a newly resolved slug. Returns the list of all token IDs to subscribe.
    /// `clob_tokens` maps condition_id -> Vec<TokenInfo> from the CLOB `/markets` endpoint,
    /// which provides authoritative token-outcome mappings. Falls back to Gamma if unavailable.
    pub async fn activate(
        &self,
        slug: &str,
        resolved: &ResolvedSlug,
        clob_tokens: &HashMap<String, Vec<TokenInfo>>,
        initial_token_states: HashMap<String, TokenState>,
    ) -> Vec<String> {
        let markets: Vec<MarketMeta> = resolved
            .markets
            .iter()
            .map(|m| MarketMeta {
                question: m.question.as_deref().unwrap_or("").to_string(),
                condition_id: m.condition_id.clone(),
                tokens: clob_tokens
                    .get(&m.condition_id)
                    .cloned()
                    .unwrap_or_else(|| m.token_infos()),
            })
            .collect();

        let all_token_ids: Vec<String> = markets
            .iter()
            .flat_map(|m| m.tokens.iter().map(|t| t.token_id.clone()))
            .collect();

        let mut tokens = HashMap::new();
        for tid in &all_token_ids {
            let ts = initial_token_states
                .get(tid)
                .cloned()
                .unwrap_or_default();
            tokens.insert(tid.clone(), ts);
        }

        let state = SlugState {
            slug: slug.to_string(),
            event_title: resolved.title.clone(),
            markets,
            tokens,
            open_interest: None,
            top_holders: None,
        };

        {
            let mut slugs = self.slugs.write().await;
            slugs.insert(slug.to_string(), state);
        }
        {
            let mut t2s = self.token_to_slug.write().await;
            for tid in &all_token_ids {
                t2s.insert(tid.clone(), slug.to_string());
            }
        }

        all_token_ids
    }

    /// Get all condition IDs for a slug.
    pub async fn condition_ids_for_slug(&self, slug: &str) -> Vec<String> {
        let slugs = self.slugs.read().await;
        slugs
            .get(slug)
            .map(|s| s.markets.iter().map(|m| m.condition_id.clone()).collect())
            .unwrap_or_default()
    }

    /// Get all token IDs currently registered across all slugs.
    pub async fn all_token_ids(&self) -> Vec<String> {
        let t2s = self.token_to_slug.read().await;
        t2s.keys().cloned().collect()
    }
}

#[cfg(test)]
#[path = "tests/state.rs"]
mod tests;
