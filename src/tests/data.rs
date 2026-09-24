use super::*;

fn holder(name: Option<&str>, pseudonym: Option<&str>, wallet: Option<&str>) -> Holder {
    Holder {
        proxy_wallet: wallet.map(|s| s.to_string()),
        name: name.map(|s| s.to_string()),
        pseudonym: pseudonym.map(|s| s.to_string()),
        amount: Some(100.0),
        outcome_index: Some(0),
        profile_image: None,
    }
}

#[test]
fn display_name_prefers_name() {
    assert_eq!(
        holder(Some("Alice"), Some("Bob"), Some("0x1")).display_name(),
        "Alice"
    );
}

#[test]
fn display_name_falls_back_to_pseudonym() {
    assert_eq!(holder(None, Some("Bob"), Some("0x1")).display_name(), "Bob");
}

#[test]
fn display_name_empty_name_falls_back_to_pseudonym() {
    assert_eq!(
        holder(Some(""), Some("Bob"), Some("0x1")).display_name(),
        "Bob"
    );
}

#[test]
fn display_name_falls_back_to_wallet() {
    assert_eq!(holder(None, None, Some("0xabc")).display_name(), "0xabc");
}

#[test]
fn display_name_all_none() {
    assert_eq!(holder(None, None, None).display_name(), "unknown");
}

#[test]
fn display_name_all_empty() {
    assert_eq!(holder(Some(""), Some(""), None).display_name(), "unknown");
}

#[test]
fn serde_holder_camel_case() {
    let json = r#"{
        "proxyWallet": "0x123",
        "name": "Alice",
        "pseudonym": "alice_pm",
        "amount": 42.5,
        "outcomeIndex": 1,
        "profileImage": "https://example.com/img.png"
    }"#;
    let h: Holder = serde_json::from_str(json).unwrap();
    assert_eq!(h.proxy_wallet, Some("0x123".to_string()));
    assert_eq!(h.name, Some("Alice".to_string()));
    assert_eq!(h.amount, Some(42.5));
    assert_eq!(h.outcome_index, Some(1));
}

#[test]
fn serde_holder_missing_optional_fields() {
    let json = r#"{}"#;
    let h: Holder = serde_json::from_str(json).unwrap();
    assert_eq!(h.name, None);
    assert_eq!(h.amount, None);
    assert_eq!(h.display_name(), "unknown");
}

#[test]
fn serde_open_interest_with_null_value() {
    let json = r#"{"market": "0xabc", "value": null}"#;
    let oi: OpenInterest = serde_json::from_str(json).unwrap();
    assert_eq!(oi.market, "0xabc");
    assert_eq!(oi.value, None);
}

#[test]
fn serde_open_interest_with_value() {
    let json = r#"{"market": "0xdef", "value": 12345.67}"#;
    let oi: OpenInterest = serde_json::from_str(json).unwrap();
    assert_eq!(oi.value, Some(12345.67));
}

#[test]
fn serde_meta_holder() {
    let json = r#"{
        "token": "tok1",
        "holders": [{"name": "Alice", "amount": 100.0}]
    }"#;
    let mh: MetaHolder = serde_json::from_str(json).unwrap();
    assert_eq!(mh.token, Some("tok1".to_string()));
    let holders = mh.holders.unwrap();
    assert_eq!(holders.len(), 1);
    assert_eq!(holders[0].name, Some("Alice".to_string()));
}

#[test]
fn serde_position() {
    let json = r#"{
        "asset": "token-1",
        "conditionId": "0xcondition",
        "size": 10.0,
        "avgPrice": 0.5,
        "initialValue": 5.0,
        "currentValue": 6.5,
        "cashPnl": 1.5,
        "percentPnl": 25.0,
        "realizedPnl": 0.25,
        "curPrice": 0.65,
        "redeemable": false,
        "title": "Will it happen?",
        "outcome": "Yes"
    }"#;
    let position: Position = serde_json::from_str(json).unwrap();
    assert_eq!(position.asset, "token-1");
    assert_eq!(position.avg_price, 0.5);
    assert_eq!(position.initial_value, 5.0);
    assert_eq!(position.current_value, 6.5);
    assert_eq!(position.cash_pnl, 1.5);
    assert_eq!(position.percent_pnl, 25.0);
    assert!(!position.redeemable);
}

#[test]
fn serde_user_stats_all_time_realized_pnl() {
    let json = r#"{
        "data": {
            "all_time_pnl": {
                "realized_pnl": 1468.34
            }
        }
    }"#;
    let stats: UserStatsResponse = serde_json::from_str(json).unwrap();

    assert_eq!(
        stats
            .data
            .and_then(|stats| stats.all_time_pnl)
            .and_then(|pnl| pnl.realized_pnl),
        Some(1468.34)
    );
}

#[test]
fn serde_user_stats_all_time_pnl_can_be_unavailable() {
    let stats: UserStatsResponse = serde_json::from_str(r#"{"data": null}"#).unwrap();

    assert!(stats.data.is_none());
}
