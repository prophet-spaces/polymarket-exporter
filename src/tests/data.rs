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
fn serde_holder_snake_case() {
    let json = r#"{
        "proxy_wallet": "0x123",
        "name": "Alice",
        "pseudonym": "alice_pm",
        "amount": 42.5,
        "outcome_index": 1,
        "profile_image": "https://example.com/img.png"
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
    let json = r#"{"condition_id": "0xabc", "value": null}"#;
    let oi: OpenInterest = serde_json::from_str(json).unwrap();
    assert_eq!(oi.market, "0xabc");
    assert_eq!(oi.value, None);
}

#[test]
fn serde_open_interest_with_value() {
    let json = r#"{"condition_id": "0xdef", "value": 12345.67}"#;
    let oi: OpenInterest = serde_json::from_str(json).unwrap();
    assert_eq!(oi.value, Some(12345.67));
}

#[test]
fn serde_meta_holder() {
    let json = r#"{
        "token_id": "tok1",
        "holders": [{"name": "Alice", "amount": 100.0}]
    }"#;
    let mh: MetaHolder = serde_json::from_str(json).unwrap();
    assert_eq!(mh.token, Some("tok1".to_string()));
    let holders = mh.holders.unwrap();
    assert_eq!(holders.len(), 1);
    assert_eq!(holders[0].name, Some("Alice".to_string()));
}

const POSITION_JSON: &str = r#"{
        "token_id": "token-1",
        "condition_id": "0xcondition",
        "current_size": 10.0,
        "total_size": 20.0,
        "avg_price": 0.5,
        "entry_cost_usdc": 5.0,
        "total_cost_usdc": 5.1,
        "current_value": 6.5,
        "unrealized_pnl": 1.5,
        "total_pnl": 1.75,
        "percent_pnl": 30.0,
        "realized_pnl": 0.25,
        "current_price": 0.65,
        "redeemable": false,
        "title": "Will it happen?",
        "outcome": "Yes",
        "event_id": "123",
        "event_slug": "will-it-happen",
        "status": "OPEN",
        "first_entry_at": 100,
        "last_event_at": 200
    }"#;

#[test]
fn serde_position() {
    let position: Position = serde_json::from_str(POSITION_JSON).unwrap();
    assert_eq!(position.asset, "token-1");
    assert_eq!(position.condition_id, "0xcondition");
    assert_eq!(position.size, 10.0);
    assert_eq!(position.avg_price, 0.5);
    assert_eq!(position.initial_value, 5.0);
    assert_eq!(position.current_value, 6.5);
    assert_eq!(position.cash_pnl, 1.5);
    assert_eq!(position.percent_pnl, 30.0);
    assert_eq!(position.cur_price, 0.65);
    assert_eq!(position.realized_pnl, 0.25);
    assert!(!position.redeemable);
    assert_eq!(position.event_id, "123");
    assert_eq!(position.first_entry_at, 100);
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
    let stats: DataResponse<Option<UserStats>> = serde_json::from_str(json).unwrap();

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
    let stats: DataResponse<Option<UserStats>> = serde_json::from_str(r#"{"data": null}"#).unwrap();

    assert!(stats.data.is_none());
}

async fn mock_client(app: axum::Router) -> (DataClient, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client = DataClient {
        http: reqwest::Client::new(),
        rate_limiter: Arc::new(RateLimiterService::from_config(&Default::default())),
        base_url,
    };
    (client, server)
}

#[tokio::test]
async fn positions_follow_cursors_through_empty_pages_and_fail_on_bad_pages() {
    use axum::{extract::Query, http::StatusCode, routing::get, Json, Router};
    use serde_json::json;
    use std::collections::HashMap;

    let app = Router::new().route("/v2/positions", get(
        |Query(params): Query<HashMap<String, String>>| async move {
            let status = params["status"].as_str();
            if status == "OPEN" {
                assert_eq!(params["include_archived"], "true");
            } else {
                assert_eq!(status, "CLOSED");
                assert!(!params.contains_key("include_archived"));
                return (StatusCode::OK, Json(json!({"data": [], "pagination": {"has_more": false, "next_cursor": null}})));
            }
            assert_eq!(params["filter_type"], "TOKENS");
            assert_eq!(params["filter_amount"], "0");
            assert_eq!(params["limit"], "1000");
            assert!(!params.contains_key("offset"));
            assert!(!params.contains_key("sizeThreshold"));
            let cursor = params.get("cursor").map(String::as_str);
            let mut position: serde_json::Value = serde_json::from_str(POSITION_JSON).unwrap();
            if cursor == Some("last") {
                position["token_id"] = json!("token-2");
            }
            let body = match (params["user"].as_str(), cursor) {
                ("empty", _) => json!({"data": [], "pagination": {"has_more": false, "next_cursor": null}}),
                (_, None) => json!({"data": [position], "pagination": {"has_more": true, "next_cursor": "next/+="}}),
                ("http-error", Some(_)) => return (StatusCode::BAD_REQUEST, Json(json!({"error": "bad cursor"}))),
                ("malformed", Some(_)) => json!({"data": [{}], "pagination": {"has_more": false}}),
                ("missing-pagination", Some(_)) => json!({"data": []}),
                ("missing-cursor", Some(_)) => json!({"data": [], "pagination": {"has_more": true}}),
                ("repeated-cursor", Some(_)) => json!({"data": [], "pagination": {"has_more": true, "next_cursor": "next/+="}}),
                ("wallet", Some("next/+=")) => json!({"data": [], "pagination": {"has_more": true, "next_cursor": "last"}}),
                ("wallet", Some("last")) => json!({"data": [position], "pagination": {"has_more": false, "next_cursor": null}}),
                _ => panic!("unexpected request: {params:?}"),
            };
            (StatusCode::OK, Json(body))
        }
    ));
    let (client, server) = mock_client(app).await;
    let positions = client.get_positions("wallet").await.unwrap();
    assert_eq!(positions.len(), 2);
    assert_eq!(positions[1].cash_pnl, 1.5);
    assert!(client.get_positions("empty").await.unwrap().is_empty());
    for (wallet, message) in [
        ("http-error", "Data API /v2/positions returned 400"),
        ("malformed", "failed to deserialize wallet positions"),
        (
            "missing-pagination",
            "failed to deserialize wallet positions",
        ),
        ("missing-cursor", "inconsistent pagination"),
        ("repeated-cursor", "repeated cursor"),
    ] {
        let error = client.get_positions(wallet).await.unwrap_err();
        assert!(error.to_string().contains(message), "{wallet}: {error}");
    }
    server.abort();
}

#[tokio::test]
async fn positions_keep_closed_history_and_reject_partial_or_ambiguous_books() {
    use axum::{extract::Query, http::StatusCode, routing::get, Json, Router};
    use serde_json::json;
    use std::collections::HashMap;

    let app = Router::new().route("/v2/positions", get(
        |Query(params): Query<HashMap<String, String>>| async move {
            let mut position: serde_json::Value = serde_json::from_str(POSITION_JSON).unwrap();
            let wallet = params["user"].as_str();
            if params["status"] == "CLOSED" {
                if wallet == "closed-error" {
                    return (StatusCode::BAD_GATEWAY, Json(json!({"error": "unavailable"})));
                }
                position["status"] = json!("CLOSED");
                position["current_size"] = json!(0.0);
                position["current_value"] = json!(0.0);
                position["unrealized_pnl"] = json!(0.0);
                position["realized_pnl"] = json!(-50.0);
                if wallet != "duplicate" {
                    position["token_id"] = json!("closed-token");
                }
            }
            if wallet == "missing-event" { position["event_id"] = json!(""); }
            if wallet == "missing-field" { position.as_object_mut().unwrap().remove("first_entry_at"); }
            (StatusCode::OK, Json(json!({"data": [position], "pagination": {"has_more": false, "next_cursor": null}})))
        }
    ));
    let (client, server) = mock_client(app).await;
    let positions = client.get_positions("wallet").await.unwrap();
    assert_eq!(positions.len(), 2);
    assert_eq!(positions[1].status, "CLOSED");
    assert_eq!(positions[1].realized_pnl, -50.0);
    for (wallet, message) in [
        ("closed-error", "Data API /v2/positions returned 502"),
        ("duplicate", "appeared twice"),
        ("missing-event", "no parent event metadata"),
        ("missing-field", "failed to deserialize wallet positions"),
    ] {
        let error = client.get_positions(wallet).await.unwrap_err();
        assert!(error.to_string().contains(message), "{wallet}: {error}");
    }
    server.abort();
}

#[tokio::test]
async fn positions_reject_reopening_or_closing_during_history_read() {
    use axum::{extract::Query, routing::get, Json, Router};
    use serde_json::json;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};

    for initially_open in [false, true] {
        let reads = Arc::new(AtomicUsize::new(0));
        let app = Router::new().route("/v2/positions", get(
            move |Query(params): Query<HashMap<String, String>>| {
                let reads = reads.clone();
                async move {
                    let mut positions = Vec::new();
                    if params["status"] == "OPEN" {
                        let first_read = reads.fetch_add(1, Ordering::SeqCst) == 0;
                        if first_read == initially_open {
                            positions.push(serde_json::from_str::<serde_json::Value>(POSITION_JSON).unwrap());
                        }
                    }
                    Json(json!({"data": positions, "pagination": {"has_more": false, "next_cursor": null}}))
                }
            }
        ));
        let (client, server) = mock_client(app).await;
        let error = client.get_positions("wallet").await.unwrap_err();
        assert!(
            error.to_string().contains("open positions changed"),
            "{error}"
        );
        server.abort();
    }
}

#[tokio::test]
async fn market_reads_batch_conditions_and_keep_only_top_holder_page() {
    use axum::{
        extract::{OriginalUri, Query},
        routing::get,
        Json, Router,
    };
    use serde_json::json;
    use std::collections::HashMap;

    let handler = |OriginalUri(uri): OriginalUri, Query(params): Query<HashMap<String, String>>| async move {
        assert!(!params.contains_key("market"));
        assert!(!params.contains_key("offset"));
        assert!(!params.contains_key("cursor"));
        let conditions: Vec<_> = params["condition"].split(',').collect();
        assert!(!conditions.is_empty() && conditions.len() <= 20);
        if uri.path() == "/v2/oi" {
            assert_eq!(params.len(), 1);
            Json(
                json!({"data": conditions.iter().map(|id| json!({"condition_id": id, "value": 42.0})).collect::<Vec<_>>()}),
            )
        } else {
            assert_eq!(params["limit"], "20");
            Json(json!({
                "data": conditions.iter().map(|id| json!({"token_id": id, "holders": [{"proxy_wallet": "0x123", "amount": 42.0}]})).collect::<Vec<_>>(),
                "pagination": {"has_more": true, "next_cursor": "not-needed"}
            }))
        }
    };
    let app = Router::new()
        .route("/v2/oi", get(handler))
        .route("/v2/holders", get(handler));
    let (client, server) = mock_client(app).await;
    let ids: Vec<String> = (0..41).map(|i| format!("0x{i:064x}")).collect();
    let refs: Vec<&str> = ids.iter().map(String::as_str).collect();
    let oi = client.get_open_interest(&refs).await.unwrap();
    assert_eq!(
        oi.iter().map(|row| row.market.as_str()).collect::<Vec<_>>(),
        refs
    );
    assert_eq!(oi[40].value, Some(42.0));
    let holders = client.get_top_holders(&refs).await.unwrap();
    assert_eq!(
        holders
            .iter()
            .map(|row| row.token.as_deref().unwrap())
            .collect::<Vec<_>>(),
        refs
    );
    assert_eq!(
        holders[40].holders.as_ref().unwrap()[0].display_name(),
        "0x123"
    );
    assert!(client.get_open_interest(&[]).await.unwrap().is_empty());
    assert!(client.get_top_holders(&[]).await.unwrap().is_empty());
    server.abort();
}

#[tokio::test]
async fn data_reads_accept_empty_results_and_surface_http_and_decode_errors() {
    use axum::{extract::Query, http::StatusCode, routing::get, Router};
    use std::collections::HashMap;

    let handler = |Query(params): Query<HashMap<String, String>>| async move {
        match params
            .get("condition")
            .or_else(|| params.get("user"))
            .unwrap()
            .as_str()
        {
            "empty" => (StatusCode::OK, r#"{"data":[]}"#),
            "unknown-user" => (StatusCode::OK, r#"{"data":null}"#),
            "known-user" => (
                StatusCode::OK,
                r#"{"data":{"all_time_pnl":{"realized_pnl":12.5}}}"#,
            ),
            "no-pnl" => (StatusCode::OK, r#"{"data":{"all_time_pnl":null}}"#),
            "malformed" => (StatusCode::OK, r#"{"data":"invalid"}"#),
            _ => (
                StatusCode::SERVICE_UNAVAILABLE,
                r#"{"error":"unavailable"}"#,
            ),
        }
    };
    let app = Router::new()
        .route("/v2/oi", get(handler))
        .route("/v2/holders", get(handler))
        .route("/v2/user-stats", get(handler));
    let (client, server) = mock_client(app).await;
    assert!(client
        .get_open_interest(&["empty"])
        .await
        .unwrap()
        .is_empty());
    assert!(client.get_top_holders(&["empty"]).await.unwrap().is_empty());
    assert_eq!(
        client
            .get_all_time_realized_pnl("unknown-user")
            .await
            .unwrap(),
        None
    );
    assert_eq!(
        client.get_all_time_realized_pnl("no-pnl").await.unwrap(),
        None
    );
    assert_eq!(
        client
            .get_all_time_realized_pnl("known-user")
            .await
            .unwrap(),
        Some(12.5)
    );
    for input in ["malformed", "http-error"] {
        assert!(client.get_open_interest(&[input]).await.is_err());
        assert!(client.get_top_holders(&[input]).await.is_err());
        assert!(client.get_all_time_realized_pnl(input).await.is_err());
    }
    server.abort();
}
