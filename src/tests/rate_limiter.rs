use super::*;
use crate::config::RateLimitsConfig;

#[test]
fn from_default_config_creates_all_limiters() {
    let cfg = RateLimitsConfig::default();
    let svc = RateLimiterService::from_config(&cfg);
    assert_eq!(svc.limiter_count(), 8);
}

#[test]
fn from_default_config_has_expected_keys() {
    let cfg = RateLimitsConfig::default();
    let svc = RateLimiterService::from_config(&cfg);
    assert!(svc.has_limiter("gamma.events"));
    assert!(svc.has_limiter("clob.book"));
    assert!(svc.has_limiter("clob.fee_rate"));
    assert!(svc.has_limiter("clob.tick_size"));
    assert!(svc.has_limiter("clob.spread"));
    assert!(svc.has_limiter("data_api.general"));
    assert!(svc.has_limiter("data_api.holders"));
    assert!(svc.has_limiter("clob.markets"));
}

#[test]
fn from_empty_config_creates_no_limiters() {
    let cfg = RateLimitsConfig {
        gamma: HashMap::new(),
        clob: HashMap::new(),
        data_api: HashMap::new(),
    };
    let svc = RateLimiterService::from_config(&cfg);
    assert_eq!(svc.limiter_count(), 0);
}

#[tokio::test]
async fn acquire_unknown_key_completes_immediately() {
    let cfg = RateLimitsConfig::default();
    let svc = RateLimiterService::from_config(&cfg);
    svc.acquire("unknown", "endpoint").await;
}

#[tokio::test]
async fn acquire_known_key_succeeds() {
    let cfg = RateLimitsConfig::default();
    let svc = RateLimiterService::from_config(&cfg);
    svc.acquire("gamma", "events").await;
}
