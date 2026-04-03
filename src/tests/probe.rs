use super::*;
use crate::api::data::Holder;
use crate::api::gamma::TokenInfo;
use crate::config::CacheConfig;
use crate::state::{MarketMeta, SlugRegistry, SlugState, TokenState};
use std::collections::HashMap;

// --- escape_label tests ---

#[test]
fn escape_label_clean_string() {
    assert_eq!(escape_label("hello world"), "hello world");
}

#[test]
fn escape_label_backslash() {
    assert_eq!(escape_label("back\\slash"), "back\\\\slash");
}

#[test]
fn escape_label_double_quote() {
    assert_eq!(escape_label(r#"say "hi""#), r#"say \"hi\""#);
}

#[test]
fn escape_label_newline() {
    assert_eq!(escape_label("line1\nline2"), "line1\\nline2");
}

#[test]
fn escape_label_combined() {
    assert_eq!(escape_label("a\\b\"c\nd"), "a\\\\b\\\"c\\nd");
}

// --- render_metrics tests ---

fn make_registry_with_state(slug: &str, state: SlugState) -> SlugRegistry {
    let mut slugs = HashMap::new();
    slugs.insert(slug.to_string(), state);
    SlugRegistry {
        slugs: tokio::sync::RwLock::new(slugs),
        token_to_slug: tokio::sync::RwLock::new(HashMap::new()),
        cache_config: CacheConfig::default(),
    }
}

#[tokio::test]
async fn render_metrics_empty_registry() {
    let reg = SlugRegistry::new(CacheConfig::default());
    let out = render_metrics("nonexistent", &reg).await;
    assert!(out.is_empty());
}

#[tokio::test]
async fn render_metrics_has_help_type_headers() {
    let state = SlugState::new_for_test(
        "test",
        vec![MarketMeta {
            question: "Q".to_string(),
            condition_id: "0xc".to_string(),
            tokens: vec![TokenInfo { token_id: "t1".to_string(), outcome: "Yes".to_string() }],
        }],
        HashMap::from([("t1".to_string(), TokenState {
            best_bid: Some(0.5),
            ..Default::default()
        })]),
    );
    let reg = make_registry_with_state("s", state);
    let out = render_metrics("s", &reg).await;
    assert!(out.contains("# HELP polymarket_spread_bid"));
    assert!(out.contains("# TYPE polymarket_spread_bid gauge"));
}

#[tokio::test]
async fn render_metrics_all_token_fields() {
    let state = SlugState::new_for_test(
        "test",
        vec![MarketMeta {
            question: "Will it?".to_string(),
            condition_id: "0xc".to_string(),
            tokens: vec![TokenInfo { token_id: "t1".to_string(), outcome: "Yes".to_string() }],
        }],
        HashMap::from([("t1".to_string(), TokenState {
            best_bid: Some(0.45),
            best_ask: Some(0.55),
            spread: Some(0.1),
            last_trade_price: Some(0.50),
            tick_size: Some(0.01),
            min_order_size: Some(1.0),
            fee_rate_bps: Some(30),
        })]),
    );
    let reg = make_registry_with_state("s", state);
    let out = render_metrics("s", &reg).await;

    assert!(out.contains("polymarket_spread_bid{"));
    assert!(out.contains("polymarket_spread_ask{"));
    assert!(out.contains("polymarket_spread{"));
    assert!(out.contains("polymarket_last_trade_price{"));
    assert!(out.contains("polymarket_tick_size{"));
    assert!(out.contains("polymarket_min_order_size{"));
    assert!(out.contains("polymarket_fee_rate_bps{"));
    assert!(out.contains("0.45"));
    assert!(out.contains("0.55"));
}

#[tokio::test]
async fn render_metrics_top_holders_rank() {
    use crate::api::data::MetaHolder;

    let mut state = SlugState::new_for_test(
        "test",
        vec![MarketMeta {
            question: "Q".to_string(),
            condition_id: "0xc".to_string(),
            tokens: vec![TokenInfo { token_id: "t1".to_string(), outcome: "Yes".to_string() }],
        }],
        HashMap::from([("t1".to_string(), TokenState::default())]),
    );
    state.set_top_holders(vec![MetaHolder {
        token: Some("t1".to_string()),
        holders: Some(vec![
            Holder {
                proxy_wallet: None, name: Some("Alice".to_string()),
                pseudonym: None, amount: Some(100.0),
                outcome_index: None, profile_image: None,
            },
            Holder {
                proxy_wallet: None, name: Some("Bob".to_string()),
                pseudonym: None, amount: Some(50.0),
                outcome_index: None, profile_image: None,
            },
        ]),
    }]);

    let reg = make_registry_with_state("s", state);
    let out = render_metrics("s", &reg).await;

    assert!(out.contains(r#"holder_rank="1""#));
    assert!(out.contains(r#"holder_rank="2""#));
    assert!(out.contains(r#"holder_name="Alice""#));
    assert!(out.contains(r#"holder_name="Bob""#));
}
