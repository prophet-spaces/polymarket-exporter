use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Config {
    pub server: ServerConfig,
    pub cache: CacheConfig,
    pub rate_limits: RateLimitsConfig,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ServerConfig {
    pub listen_addr: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct CacheConfig {
    pub holders_ttl_secs: u64,
    pub open_interest_ttl_secs: u64,
    pub slug_resolution_ttl_secs: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct RateLimitsConfig {
    pub gamma: HashMap<String, RateLimitEntry>,
    pub clob: HashMap<String, RateLimitEntry>,
    pub data_api: HashMap<String, RateLimitEntry>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RateLimitEntry {
    pub max_requests: u32,
    pub window_secs: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            server: ServerConfig::default(),
            cache: CacheConfig::default(),
            rate_limits: RateLimitsConfig::default(),
        }
    }
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            listen_addr: "0.0.0.0:9184".to_string(),
        }
    }
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            holders_ttl_secs: 60,
            open_interest_ttl_secs: 30,
            slug_resolution_ttl_secs: 3600,
        }
    }
}

impl Default for RateLimitsConfig {
    fn default() -> Self {
        let gamma = HashMap::from([
            ("events".into(), RateLimitEntry { max_requests: 500, window_secs: 10 }),
        ]);
        let clob = HashMap::from([
            ("book".into(), RateLimitEntry { max_requests: 1500, window_secs: 10 }),
            ("fee_rate".into(), RateLimitEntry { max_requests: 200, window_secs: 10 }),
            ("tick_size".into(), RateLimitEntry { max_requests: 200, window_secs: 10 }),
            ("spread".into(), RateLimitEntry { max_requests: 1500, window_secs: 10 }),
            ("markets".into(), RateLimitEntry { max_requests: 500, window_secs: 10 }),
        ]);
        let data_api = HashMap::from([
            ("general".into(), RateLimitEntry { max_requests: 1000, window_secs: 10 }),
            ("holders".into(), RateLimitEntry { max_requests: 150, window_secs: 10 }),
        ]);
        Self { gamma, clob, data_api }
    }
}

impl Config {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let contents = std::fs::read_to_string(path)?;
        let config: Config = toml::from_str(&contents)?;
        Ok(config)
    }
}

#[cfg(test)]
#[path = "tests/config.rs"]
mod tests;
