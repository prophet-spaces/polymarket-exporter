use super::*;

fn make_market(clob_token_ids: Option<&str>, outcomes: Option<&str>) -> GammaMarket {
    GammaMarket {
        id: "1".to_string(),
        question: Some("Will X happen?".to_string()),
        condition_id: "0xabc".to_string(),
        slug: Some("will-x-happen".to_string()),
        outcomes: outcomes.map(|s| s.to_string()),
        clob_token_ids: clob_token_ids.map(|s| s.to_string()),
        active: Some(true),
        closed: Some(false),
        order_price_min_tick_size: Some(0.01),
        order_min_size: Some(1.0),
    }
}

#[test]
fn token_infos_both_present() {
    let m = make_market(
        Some(r#"["token_a","token_b"]"#),
        Some(r#"["Yes","No"]"#),
    );
    let infos = m.token_infos();
    assert_eq!(infos.len(), 2);
    assert_eq!(infos[0].token_id, "token_a");
    assert_eq!(infos[0].outcome, "Yes");
    assert_eq!(infos[1].token_id, "token_b");
    assert_eq!(infos[1].outcome, "No");
}

#[test]
fn token_infos_more_tokens_than_outcomes() {
    let m = make_market(
        Some(r#"["t1","t2","t3"]"#),
        Some(r#"["Yes"]"#),
    );
    let infos = m.token_infos();
    assert_eq!(infos.len(), 3);
    assert_eq!(infos[0].outcome, "Yes");
    assert_eq!(infos[1].outcome, "Unknown");
    assert_eq!(infos[2].outcome, "Unknown");
}

#[test]
fn token_infos_both_none() {
    let m = make_market(None, None);
    assert!(m.token_infos().is_empty());
}

#[test]
fn token_infos_tokens_none_outcomes_some() {
    let m = make_market(None, Some(r#"["Yes","No"]"#));
    assert!(m.token_infos().is_empty());
}

#[test]
fn token_infos_invalid_json() {
    let m = make_market(Some("not json"), Some("also not json"));
    assert!(m.token_infos().is_empty());
}

#[test]
fn token_infos_single_token() {
    let m = make_market(
        Some(r#"["only_one"]"#),
        Some(r#"["Yes"]"#),
    );
    let infos = m.token_infos();
    assert_eq!(infos.len(), 1);
    assert_eq!(infos[0].token_id, "only_one");
    assert_eq!(infos[0].outcome, "Yes");
}

#[test]
fn serde_gamma_market_camel_case() {
    let json = r#"{
        "id": "123",
        "question": "Will it rain?",
        "conditionId": "0xdef",
        "slug": "will-it-rain",
        "outcomes": "[\"Yes\",\"No\"]",
        "clobTokenIds": "[\"t1\",\"t2\"]",
        "active": true,
        "closed": false,
        "orderPriceMinTickSize": 0.001,
        "orderMinSize": 5.0
    }"#;
    let m: GammaMarket = serde_json::from_str(json).unwrap();
    assert_eq!(m.id, "123");
    assert_eq!(m.condition_id, "0xdef");
    assert_eq!(m.order_price_min_tick_size, Some(0.001));
    assert_eq!(m.token_infos().len(), 2);
}

#[test]
fn serde_gamma_event() {
    let json = r#"{
        "id": "evt1",
        "slug": "some-event",
        "title": "Some Event",
        "markets": []
    }"#;
    let e: GammaEvent = serde_json::from_str(json).unwrap();
    assert_eq!(e.id, "evt1");
    assert_eq!(e.title, Some("Some Event".to_string()));
    assert_eq!(e.markets.unwrap().len(), 0);
}
