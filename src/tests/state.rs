use super::*;
use crate::api::gamma::GammaMarket;
use std::time::{Duration, Instant};

fn make_slug_state() -> SlugState {
    SlugState {
        slug: "test-slug".to_string(),
        event_title: "Test Event".to_string(),
        markets: vec![MarketMeta {
            question: "Will X?".to_string(),
            condition_id: "0xcond1".to_string(),
            tokens: vec![
                TokenInfo { token_id: "tok1".to_string(), outcome: "Yes".to_string() },
                TokenInfo { token_id: "tok2".to_string(), outcome: "No".to_string() },
            ],
        }],
        tokens: HashMap::from([
            ("tok1".to_string(), TokenState::default()),
            ("tok2".to_string(), TokenState::default()),
        ]),
        open_interest: None,
        top_holders: None,
    }
}

#[test]
fn cached_data_fresh_is_not_expired() {
    let cached = CachedData {
        data: 42,
        fetched_at: Instant::now(),
    };
    assert!(!cached.is_expired(Duration::from_secs(60)));
}

#[test]
fn cached_data_old_is_expired() {
    let cached = CachedData {
        data: 42,
        fetched_at: Instant::now() - Duration::from_secs(120),
    };
    assert!(cached.is_expired(Duration::from_secs(60)));
}

#[test]
fn get_open_interest_none_when_empty() {
    let ss = make_slug_state();
    assert_eq!(ss.get_open_interest("0xcond1"), None);
}

#[test]
fn is_oi_expired_true_when_none() {
    let ss = make_slug_state();
    assert!(ss.is_oi_expired(Duration::from_secs(30)));
}

#[test]
fn set_and_get_open_interest() {
    let mut ss = make_slug_state();
    ss.set_open_interest(vec![
        OpenInterest { market: "0xcond1".to_string(), value: Some(1000.0) },
        OpenInterest { market: "0xcond2".to_string(), value: None },
    ]);
    assert_eq!(ss.get_open_interest("0xcond1"), Some(1000.0));
    assert_eq!(ss.get_open_interest("0xcond2"), None);
}

#[test]
fn oi_not_expired_after_set() {
    let mut ss = make_slug_state();
    ss.set_open_interest(vec![]);
    assert!(!ss.is_oi_expired(Duration::from_secs(60)));
}

#[test]
fn get_top_holders_none_when_empty() {
    let ss = make_slug_state();
    assert!(ss.get_top_holders("tok1").is_none());
}

#[test]
fn is_holders_expired_true_when_none() {
    let ss = make_slug_state();
    assert!(ss.is_holders_expired(Duration::from_secs(60)));
}

#[test]
fn set_and_get_top_holders() {
    let mut ss = make_slug_state();
    ss.set_top_holders(vec![
        MetaHolder {
            token: Some("tok1".to_string()),
            holders: Some(vec![Holder {
                proxy_wallet: Some("0x1".to_string()),
                name: Some("Alice".to_string()),
                pseudonym: None,
                amount: Some(500.0),
                outcome_index: Some(0),
                profile_image: None,
            }]),
        },
        MetaHolder { token: None, holders: Some(vec![]) },
        MetaHolder { token: Some("tok_missing".to_string()), holders: None },
    ]);
    let holders = ss.get_top_holders("tok1").unwrap();
    assert_eq!(holders.len(), 1);
    assert_eq!(holders[0].name, Some("Alice".to_string()));
    assert!(ss.get_top_holders("tok_missing").is_none());
}

#[tokio::test]
async fn registry_not_active_by_default() {
    let reg = SlugRegistry::new(CacheConfig::default());
    assert!(!reg.is_active("anything").await);
}

#[tokio::test]
async fn registry_activate_and_is_active() {
    let reg = SlugRegistry::new(CacheConfig::default());
    let resolved = ResolvedSlug {
        title: "Test".to_string(),
        markets: vec![GammaMarket {
            id: "m1".to_string(),
            question: Some("Will X?".to_string()),
            condition_id: "0xcond".to_string(),
            slug: Some("test-slug".to_string()),
            outcomes: Some(r#"["Yes","No"]"#.to_string()),
            clob_token_ids: Some(r#"["tok_a","tok_b"]"#.to_string()),
            active: Some(true),
            closed: Some(false),
            order_price_min_tick_size: None,
            order_min_size: None,
        }],
    };

    let token_ids = reg.activate("test-slug", &resolved, &HashMap::new(), HashMap::new()).await;

    assert!(reg.is_active("test-slug").await);
    assert_eq!(token_ids.len(), 2);
    assert!(token_ids.contains(&"tok_a".to_string()));
    assert!(token_ids.contains(&"tok_b".to_string()));
}

#[tokio::test]
async fn registry_token_to_slug_mapping() {
    let reg = SlugRegistry::new(CacheConfig::default());
    let resolved = ResolvedSlug {
        title: "T".to_string(),
        markets: vec![GammaMarket {
            id: "m1".to_string(),
            question: Some("Q".to_string()),
            condition_id: "0xc".to_string(),
            slug: None,
            outcomes: Some(r#"["Yes"]"#.to_string()),
            clob_token_ids: Some(r#"["tok_x"]"#.to_string()),
            active: None,
            closed: None,
            order_price_min_tick_size: None,
            order_min_size: None,
        }],
    };

    reg.activate("my-slug", &resolved, &HashMap::new(), HashMap::new()).await;

    let t2s = reg.token_to_slug.read().await;
    assert_eq!(t2s.get("tok_x"), Some(&"my-slug".to_string()));
}

#[tokio::test]
async fn registry_condition_ids_for_slug() {
    let reg = SlugRegistry::new(CacheConfig::default());
    let resolved = ResolvedSlug {
        title: "T".to_string(),
        markets: vec![
            GammaMarket {
                id: "m1".to_string(), question: None, condition_id: "0xaaa".to_string(),
                slug: None, outcomes: None, clob_token_ids: None,
                active: None, closed: None,
                order_price_min_tick_size: None, order_min_size: None,
            },
            GammaMarket {
                id: "m2".to_string(), question: None, condition_id: "0xbbb".to_string(),
                slug: None, outcomes: None, clob_token_ids: None,
                active: None, closed: None,
                order_price_min_tick_size: None, order_min_size: None,
            },
        ],
    };

    reg.activate("multi", &resolved, &HashMap::new(), HashMap::new()).await;
    let cids = reg.condition_ids_for_slug("multi").await;
    assert_eq!(cids.len(), 2);
    assert!(cids.contains(&"0xaaa".to_string()));
    assert!(cids.contains(&"0xbbb".to_string()));
}

#[tokio::test]
async fn registry_condition_ids_unknown_slug() {
    let reg = SlugRegistry::new(CacheConfig::default());
    assert!(reg.condition_ids_for_slug("nope").await.is_empty());
}

#[tokio::test]
async fn registry_activate_prefers_clob_tokens() {
    let reg = SlugRegistry::new(CacheConfig::default());
    let resolved = ResolvedSlug {
        title: "Test".to_string(),
        markets: vec![GammaMarket {
            id: "m1".to_string(),
            question: Some("Will X?".to_string()),
            condition_id: "0xcond".to_string(),
            slug: Some("test-slug".to_string()),
            // Gamma has outcomes in wrong order
            outcomes: Some(r#"["No","Yes"]"#.to_string()),
            clob_token_ids: Some(r#"["tok_a","tok_b"]"#.to_string()),
            active: Some(true),
            closed: Some(false),
            order_price_min_tick_size: None,
            order_min_size: None,
        }],
    };

    // CLOB provides the correct mapping
    let clob_tokens = HashMap::from([(
        "0xcond".to_string(),
        vec![
            TokenInfo { token_id: "tok_a".to_string(), outcome: "Yes".to_string() },
            TokenInfo { token_id: "tok_b".to_string(), outcome: "No".to_string() },
        ],
    )]);

    reg.activate("test-slug", &resolved, &clob_tokens, HashMap::new()).await;

    let slugs = reg.slugs.read().await;
    let ss = slugs.get("test-slug").unwrap();
    let tokens = &ss.markets[0].tokens;
    // Should use CLOB mapping (Yes, No) not Gamma mapping (No, Yes)
    assert_eq!(tokens[0].outcome, "Yes");
    assert_eq!(tokens[0].token_id, "tok_a");
    assert_eq!(tokens[1].outcome, "No");
    assert_eq!(tokens[1].token_id, "tok_b");
}

#[tokio::test]
async fn registry_activate_falls_back_to_gamma_tokens() {
    let reg = SlugRegistry::new(CacheConfig::default());
    let resolved = ResolvedSlug {
        title: "Test".to_string(),
        markets: vec![GammaMarket {
            id: "m1".to_string(),
            question: Some("Will X?".to_string()),
            condition_id: "0xcond".to_string(),
            slug: Some("test-slug".to_string()),
            outcomes: Some(r#"["Yes","No"]"#.to_string()),
            clob_token_ids: Some(r#"["tok_a","tok_b"]"#.to_string()),
            active: Some(true),
            closed: Some(false),
            order_price_min_tick_size: None,
            order_min_size: None,
        }],
    };

    // Empty CLOB tokens — should fall back to Gamma
    reg.activate("test-slug", &resolved, &HashMap::new(), HashMap::new()).await;

    let slugs = reg.slugs.read().await;
    let ss = slugs.get("test-slug").unwrap();
    let tokens = &ss.markets[0].tokens;
    assert_eq!(tokens[0].outcome, "Yes");
    assert_eq!(tokens[1].outcome, "No");
}
