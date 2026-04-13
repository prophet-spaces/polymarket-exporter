use super::*;

#[test]
fn deserialize_clob_market() {
    let json = r#"{
        "tokens": [
            {"token_id": "tok_a", "outcome": "Yes"},
            {"token_id": "tok_b", "outcome": "No"}
        ]
    }"#;
    let market: ClobMarket = serde_json::from_str(json).unwrap();
    assert_eq!(market.tokens.len(), 2);
    assert_eq!(market.tokens[0].token_id, "tok_a");
    assert_eq!(market.tokens[0].outcome, "Yes");
    assert_eq!(market.tokens[1].token_id, "tok_b");
    assert_eq!(market.tokens[1].outcome, "No");
}

#[test]
fn deserialize_clob_market_ignores_extra_fields() {
    let json = r#"{
        "condition_id": "0x123",
        "question_id": "0xabc",
        "tokens": [
            {"token_id": "tok_a", "outcome": "Yes", "price": 0.5, "winner": false}
        ],
        "minimum_order_size": 5,
        "minimum_tick_size": 0.01
    }"#;
    let market: ClobMarket = serde_json::from_str(json).unwrap();
    assert_eq!(market.tokens.len(), 1);
    assert_eq!(market.tokens[0].token_id, "tok_a");
    assert_eq!(market.tokens[0].outcome, "Yes");
}

#[test]
fn deserialize_clob_market_empty_tokens() {
    let json = r#"{"tokens": []}"#;
    let market: ClobMarket = serde_json::from_str(json).unwrap();
    assert!(market.tokens.is_empty());
}
