use super::*;

#[test]
fn last_trade_price_handles_no_trades_and_validates_prices() {
    for (json, expected) in [
        (r#"{"price":"0.365","side":"BUY"}"#, Some(0.365)),
        (r#"{"price":"0.635","side":"SELL"}"#, Some(0.635)),
        (r#"{"price":"0.5","side":""}"#, None),
        (r#"{"price":"0.5","side":"BUY"}"#, Some(0.5)),
        (r#"{"price":"0","side":"SELL"}"#, Some(0.0)),
        (r#"{"price":"1","side":"BUY"}"#, Some(1.0)),
    ] {
        let trade: LastTradePriceResponse = serde_json::from_str(json).unwrap();
        assert_eq!(trade.into_price().unwrap(), expected);
    }

    for price in ["invalid", "NaN", "inf", "-0.1", "1.1"] {
        let trade = LastTradePriceResponse {
            price: price.to_string(),
            side: "BUY".to_string(),
        };
        assert!(trade.into_price().is_err(), "accepted price {price}");
    }
    let trade: LastTradePriceResponse =
        serde_json::from_str(r#"{"price":"0.5","side":"unknown"}"#).unwrap();
    assert!(trade.into_price().is_err());
}

#[tokio::test]
async fn last_trade_price_requests_each_token_and_surfaces_api_errors() {
    use axum::{extract::Query, http::StatusCode, routing::get, Router};
    use std::collections::HashMap;

    let app = Router::new().route(
        "/last-trade-price",
        get(|Query(params): Query<HashMap<String, String>>| async move {
            match params.get("token_id").map(String::as_str) {
                Some("yes") => (StatusCode::OK, r#"{"price":"0.365","side":"BUY"}"#),
                Some("no") => (StatusCode::OK, r#"{"price":"0.635","side":"SELL"}"#),
                Some("untraded") => (StatusCode::OK, r#"{"price":"0.5","side":""}"#),
                Some("malformed") => (StatusCode::OK, r#"{"price":"0.5"}"#),
                Some("invalid-price") => (StatusCode::OK, r#"{"price":"NaN","side":"BUY"}"#),
                _ => (StatusCode::BAD_REQUEST, r#"{"error":"Invalid token id"}"#),
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client = ClobClient {
        http: reqwest::Client::new(),
        rate_limiter: Arc::new(RateLimiterService::from_config(&Default::default())),
        base_url,
    };

    assert_eq!(
        client.get_last_trade_price("yes").await.unwrap(),
        Some(0.365)
    );
    assert_eq!(
        client.get_last_trade_price("no").await.unwrap(),
        Some(0.635)
    );
    assert_eq!(client.get_last_trade_price("untraded").await.unwrap(), None);
    assert!(client.get_last_trade_price("malformed").await.is_err());
    assert!(client.get_last_trade_price("invalid-price").await.is_err());
    let error = client.get_last_trade_price("unknown").await.unwrap_err();
    assert!(error
        .to_string()
        .contains("CLOB /last-trade-price returned 400"));
    server.abort();
}

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
