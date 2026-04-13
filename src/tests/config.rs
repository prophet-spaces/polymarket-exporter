use super::*;
use std::io::Write;

#[test]
fn default_server_config() {
    let cfg = ServerConfig::default();
    assert_eq!(cfg.listen_addr, "0.0.0.0:9184");
}

#[test]
fn default_cache_config() {
    let cfg = CacheConfig::default();
    assert_eq!(cfg.holders_ttl_secs, 60);
    assert_eq!(cfg.open_interest_ttl_secs, 30);
    assert_eq!(cfg.slug_resolution_ttl_secs, 3600);
}

#[test]
fn default_rate_limits_gamma() {
    let cfg = RateLimitsConfig::default();
    let events = cfg.gamma.get("events").expect("gamma.events missing");
    assert_eq!(events.max_requests, 500);
    assert_eq!(events.window_secs, 10);
}

#[test]
fn default_rate_limits_clob() {
    let cfg = RateLimitsConfig::default();
    let book = cfg.clob.get("book").expect("clob.book missing");
    assert_eq!(book.max_requests, 1500);
    assert_eq!(book.window_secs, 10);

    let fee_rate = cfg.clob.get("fee_rate").expect("clob.fee_rate missing");
    assert_eq!(fee_rate.max_requests, 200);

    let tick_size = cfg.clob.get("tick_size").expect("clob.tick_size missing");
    assert_eq!(tick_size.max_requests, 200);

    let spread = cfg.clob.get("spread").expect("clob.spread missing");
    assert_eq!(spread.max_requests, 1500);

    let markets = cfg.clob.get("markets").expect("clob.markets missing");
    assert_eq!(markets.max_requests, 500);
    assert_eq!(markets.window_secs, 10);
}

#[test]
fn default_rate_limits_data_api() {
    let cfg = RateLimitsConfig::default();
    let general = cfg.data_api.get("general").expect("data_api.general missing");
    assert_eq!(general.max_requests, 1000);
    assert_eq!(general.window_secs, 10);

    let holders = cfg.data_api.get("holders").expect("data_api.holders missing");
    assert_eq!(holders.max_requests, 150);
}

#[test]
fn load_valid_toml() {
    let mut tmpfile = tempfile::NamedTempFile::new().unwrap();
    write!(
        tmpfile,
        r#"
[server]
listen_addr = "127.0.0.1:3000"

[cache]
holders_ttl_secs = 120
"#
    )
    .unwrap();

    let cfg = Config::load(tmpfile.path()).unwrap();
    assert_eq!(cfg.server.listen_addr, "127.0.0.1:3000");
    assert_eq!(cfg.cache.holders_ttl_secs, 120);
    // Unspecified fields use defaults
    assert_eq!(cfg.cache.open_interest_ttl_secs, 30);
}

#[test]
fn load_empty_toml_uses_defaults() {
    let mut tmpfile = tempfile::NamedTempFile::new().unwrap();
    write!(tmpfile, "").unwrap();

    let cfg = Config::load(tmpfile.path()).unwrap();
    assert_eq!(cfg.server.listen_addr, "0.0.0.0:9184");
    assert_eq!(cfg.cache.holders_ttl_secs, 60);
}

#[test]
fn load_invalid_toml_errors() {
    let mut tmpfile = tempfile::NamedTempFile::new().unwrap();
    write!(tmpfile, "{{{{ not valid toml").unwrap();

    assert!(Config::load(tmpfile.path()).is_err());
}

#[test]
fn load_with_rate_limit_overrides() {
    let mut tmpfile = tempfile::NamedTempFile::new().unwrap();
    write!(
        tmpfile,
        r#"
[rate_limits.gamma]
events = {{ max_requests = 100, window_secs = 5 }}
"#
    )
    .unwrap();

    let cfg = Config::load(tmpfile.path()).unwrap();
    let events = cfg.rate_limits.gamma.get("events").unwrap();
    assert_eq!(events.max_requests, 100);
    assert_eq!(events.window_secs, 5);
}
