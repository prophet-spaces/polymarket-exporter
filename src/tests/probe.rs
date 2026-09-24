use super::*;
use crate::api::data::{Holder, Position};
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

#[test]
fn wallet_address_requires_40_hex_characters() {
    assert!(is_wallet_address(
        "0x0123456789abcdef0123456789abcdef01234567"
    ));
    assert!(!is_wallet_address("0x0123"));
    assert!(!is_wallet_address(
        "0123456789abcdef0123456789abcdef01234567"
    ));
    assert!(!is_wallet_address(
        "0x0123456789abcdef0123456789abcdef0123456z"
    ));
}

#[test]
fn render_wallet_metrics_has_position_values() {
    let positions = vec![Position {
        asset: "token-1".to_string(),
        condition_id: "0xcondition".to_string(),
        size: 10.0,
        avg_price: 0.5,
        initial_value: 5.0,
        current_value: 6.5,
        cash_pnl: 1.5,
        percent_pnl: 25.0,
        realized_pnl: 0.25,
        cur_price: 0.65,
        redeemable: false,
        title: "Will it happen?".to_string(),
        outcome: "Yes".to_string(),
    }];
    let out = render_wallet_metrics(
        "0x0123456789abcdef0123456789abcdef01234567",
        &positions,
        Some(12.5),
    );

    assert!(out.contains("# HELP polymarket_wallet_position_current_value_usdc"));
    assert!(out.contains("polymarket_wallet_position_shares{"));
    assert!(out.contains("token_id=\"token-1\""));
    assert!(out.contains("polymarket_wallet_position_cash_pnl_usdc"));
    assert!(out.contains("polymarket_wallet_position_average_price{") && out.contains(" 0.5"));
    assert!(out.contains("polymarket_wallet_position_traded_usdc{") && out.contains(" 5"));
    assert!(out.contains("polymarket_wallet_position_payout_if_win_usdc{") && out.contains(" 10"));
    assert!(
        out.contains("polymarket_wallet_position_unrealized_gain_usdc{") && out.contains(" 1.5")
    );
    assert!(out.lines().any(|line| {
        line.starts_with("polymarket_wallet_position_unrealized_gain_ratio{")
            && line.ends_with(" 0.3")
    }));
    assert!(out.lines().any(
        |line| line.starts_with("polymarket_wallet_position_pnl_ratio{") && line.ends_with(" 0.25")
    ));
    assert!(out.contains(
        "polymarket_wallet_all_time_realized_pnl_usdc{wallet=\"0x0123456789abcdef0123456789abcdef01234567\"} 12.5"
    ));
    assert!(out.contains(" 1.5"));
}

#[test]
fn render_wallet_metrics_excludes_redeemable_positions() {
    let positions = vec![Position {
        asset: "settled-token".to_string(),
        condition_id: "0xcondition".to_string(),
        size: 10.0,
        avg_price: 1.0,
        initial_value: 10.0,
        current_value: 0.0,
        cash_pnl: -10.0,
        percent_pnl: -100.0,
        realized_pnl: 0.0,
        cur_price: 0.0,
        redeemable: true,
        title: "Settled market".to_string(),
        outcome: "No".to_string(),
    }];

    let out = render_wallet_metrics(
        "0x0123456789abcdef0123456789abcdef01234567",
        &positions,
        Some(12.5),
    );

    assert!(!out.contains("settled-token"));
    assert!(!out.contains("Settled market"));
    assert!(out.contains("polymarket_wallet_all_time_realized_pnl_usdc"));
}

// --- render_market_metrics tests ---

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
async fn render_market_metrics_empty_registry() {
    let reg = SlugRegistry::new(CacheConfig::default());
    let out = render_market_metrics("nonexistent", &reg).await;
    assert!(out.is_empty());
}

#[tokio::test]
async fn render_market_metrics_has_help_type_headers() {
    let state = SlugState::new_for_test(
        "test",
        vec![MarketMeta {
            question: "Q".to_string(),
            condition_id: "0xc".to_string(),
            tokens: vec![TokenInfo {
                token_id: "t1".to_string(),
                outcome: "Yes".to_string(),
            }],
        }],
        HashMap::from([(
            "t1".to_string(),
            TokenState {
                best_bid: Some(0.5),
                ..Default::default()
            },
        )]),
    );
    let reg = make_registry_with_state("s", state);
    let out = render_market_metrics("s", &reg).await;
    assert!(out.contains("# HELP polymarket_market_spread_bid"));
    assert!(out.contains("# TYPE polymarket_market_spread_bid gauge"));
}

#[tokio::test]
async fn render_market_metrics_all_token_fields() {
    let state = SlugState::new_for_test(
        "test",
        vec![MarketMeta {
            question: "Will it?".to_string(),
            condition_id: "0xc".to_string(),
            tokens: vec![TokenInfo {
                token_id: "t1".to_string(),
                outcome: "Yes".to_string(),
            }],
        }],
        HashMap::from([(
            "t1".to_string(),
            TokenState {
                best_bid: Some(0.45),
                best_ask: Some(0.55),
                spread: Some(0.1),
                last_trade_price: Some(0.50),
                tick_size: Some(0.01),
                min_order_size: Some(1.0),
                fee_rate_bps: Some(30),
            },
        )]),
    );
    let reg = make_registry_with_state("s", state);
    let out = render_market_metrics("s", &reg).await;

    assert!(out.contains("polymarket_market_spread_bid{"));
    assert!(out.contains("polymarket_market_spread_ask{"));
    assert!(out.contains("polymarket_market_spread{"));
    assert!(out.contains("polymarket_market_last_trade_price{"));
    assert!(out.contains("polymarket_market_tick_size{"));
    assert!(out.contains("polymarket_market_min_order_size{"));
    assert!(out.contains("polymarket_market_fee_rate_bps{"));
    assert!(out.contains("0.45"));
    assert!(out.contains("0.55"));
}

#[tokio::test]
async fn render_market_metrics_top_holders_rank() {
    use crate::api::data::MetaHolder;

    let mut state = SlugState::new_for_test(
        "test",
        vec![MarketMeta {
            question: "Q".to_string(),
            condition_id: "0xc".to_string(),
            tokens: vec![TokenInfo {
                token_id: "t1".to_string(),
                outcome: "Yes".to_string(),
            }],
        }],
        HashMap::from([("t1".to_string(), TokenState::default())]),
    );
    state.set_top_holders(vec![MetaHolder {
        token: Some("t1".to_string()),
        holders: Some(vec![
            Holder {
                proxy_wallet: None,
                name: Some("Alice".to_string()),
                pseudonym: None,
                amount: Some(100.0),
                outcome_index: None,
                profile_image: None,
            },
            Holder {
                proxy_wallet: None,
                name: Some("Bob".to_string()),
                pseudonym: None,
                amount: Some(50.0),
                outcome_index: None,
                profile_image: None,
            },
        ]),
    }]);

    let reg = make_registry_with_state("s", state);
    let out = render_market_metrics("s", &reg).await;

    assert!(out.contains(r#"holder_rank="1""#));
    assert!(out.contains(r#"holder_rank="2""#));
    assert!(out.contains(r#"holder_name="Alice""#));
    assert!(out.contains(r#"holder_name="Bob""#));
}
