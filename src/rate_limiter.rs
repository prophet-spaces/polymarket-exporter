use governor::{Quota, RateLimiter};
use nonzero_ext::nonzero;
use std::collections::HashMap;
use std::num::NonZeroU32;
use std::sync::Arc;

use crate::config::RateLimitsConfig;

type Limiter = RateLimiter<
    governor::state::NotKeyed,
    governor::state::InMemoryState,
    governor::clock::DefaultClock,
    governor::middleware::NoOpMiddleware,
>;

pub struct RateLimiterService {
    limiters: HashMap<String, Arc<Limiter>>,
}

impl RateLimiterService {
    pub fn from_config(cfg: &RateLimitsConfig) -> Self {
        let mut limiters = HashMap::new();

        for (endpoint, entry) in &cfg.gamma {
            let key = format!("gamma.{}", endpoint);
            limiters.insert(key, Self::build_limiter(entry.max_requests, entry.window_secs));
        }
        for (endpoint, entry) in &cfg.clob {
            let key = format!("clob.{}", endpoint);
            limiters.insert(key, Self::build_limiter(entry.max_requests, entry.window_secs));
        }
        for (endpoint, entry) in &cfg.data_api {
            let key = format!("data_api.{}", endpoint);
            limiters.insert(key, Self::build_limiter(entry.max_requests, entry.window_secs));
        }

        Self { limiters }
    }

    fn build_limiter(max_requests: u32, window_secs: u64) -> Arc<Limiter> {
        let nr = NonZeroU32::new(max_requests).unwrap_or(nonzero!(1u32));
        let quota = Quota::with_period(std::time::Duration::from_secs(window_secs))
            .expect("window_secs must be > 0")
            .allow_burst(nr);
        Arc::new(RateLimiter::direct(quota))
    }

    /// Wait until the rate limit allows a request for the given api and endpoint.
    /// Key format: "gamma.events", "clob.book", "data_api.holders", etc.
    pub async fn acquire(&self, api: &str, endpoint: &str) {
        let key = format!("{}.{}", api, endpoint);
        if let Some(limiter) = self.limiters.get(&key) {
            limiter.until_ready().await;
        }
    }

    #[cfg(test)]
    pub fn limiter_count(&self) -> usize {
        self.limiters.len()
    }

    #[cfg(test)]
    pub fn has_limiter(&self, key: &str) -> bool {
        self.limiters.contains_key(key)
    }
}

#[cfg(test)]
#[path = "tests/rate_limiter.rs"]
mod tests;
