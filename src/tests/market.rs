use super::*;
use crate::state::TokenState;

fn make_event(json: &str) -> WsEvent {
    serde_json::from_str(json).unwrap()
}

#[test]
fn book_event_updates_bid_ask_spread() {
    let event = make_event(r#"{
        "event_type": "book",
        "asset_id": "tok1",
        "bids": [{"price": "0.60", "size": "100"}],
        "asks": [{"price": "0.65", "size": "50"}]
    }"#);
    let mut ts = TokenState::default();
    apply_ws_event(&event, &mut ts);

    assert_eq!(ts.best_bid, Some(0.60));
    assert_eq!(ts.best_ask, Some(0.65));
    assert!((ts.spread.unwrap() - 0.05).abs() < 1e-10);
}

#[test]
fn book_event_empty_bids_asks() {
    let event = make_event(r#"{
        "event_type": "book",
        "asset_id": "tok1",
        "bids": [],
        "asks": []
    }"#);
    let mut ts = TokenState::default();
    apply_ws_event(&event, &mut ts);

    assert_eq!(ts.best_bid, None);
    assert_eq!(ts.best_ask, None);
    assert_eq!(ts.spread, None);
}

#[test]
fn best_bid_ask_event() {
    let event = make_event(r#"{
        "event_type": "best_bid_ask",
        "asset_id": "tok1",
        "best_bid": "0.42",
        "best_ask": "0.58",
        "spread": "0.16"
    }"#);
    let mut ts = TokenState::default();
    apply_ws_event(&event, &mut ts);

    assert_eq!(ts.best_bid, Some(0.42));
    assert_eq!(ts.best_ask, Some(0.58));
    assert_eq!(ts.spread, Some(0.16));
}

#[test]
fn last_trade_price_event() {
    let event = make_event(r#"{
        "event_type": "last_trade_price",
        "asset_id": "tok1",
        "price": "0.73"
    }"#);
    let mut ts = TokenState::default();
    apply_ws_event(&event, &mut ts);

    assert_eq!(ts.last_trade_price, Some(0.73));
}

#[test]
fn tick_size_change_event() {
    let event = make_event(r#"{
        "event_type": "tick_size_change",
        "asset_id": "tok1",
        "new_tick_size": "0.001"
    }"#);
    let mut ts = TokenState::default();
    apply_ws_event(&event, &mut ts);

    assert_eq!(ts.tick_size, Some(0.001));
}

#[test]
fn unknown_event_type_is_noop() {
    let event = make_event(r#"{
        "event_type": "something_else",
        "asset_id": "tok1"
    }"#);
    let mut ts = TokenState::default();
    apply_ws_event(&event, &mut ts);

    assert_eq!(ts.best_bid, None);
    assert_eq!(ts.best_ask, None);
    assert_eq!(ts.last_trade_price, None);
}

#[test]
fn book_event_picks_best_from_multiple_levels() {
    // Bids sorted ascending (worst first) and asks sorted descending (worst first),
    // mimicking Polymarket's actual orderbook ordering.
    let event = make_event(r#"{
        "event_type": "book",
        "asset_id": "tok1",
        "bids": [
            {"price": "0.001", "size": "100"},
            {"price": "0.35", "size": "200"},
            {"price": "0.38", "size": "3000"}
        ],
        "asks": [
            {"price": "0.999", "size": "100"},
            {"price": "0.42", "size": "200"},
            {"price": "0.039", "size": "800"}
        ]
    }"#);
    let mut ts = TokenState::default();
    apply_ws_event(&event, &mut ts);

    assert!((ts.best_bid.unwrap() - 0.38).abs() < 1e-10, "should pick highest bid");
    assert!((ts.best_ask.unwrap() - 0.039).abs() < 1e-10, "should pick lowest ask");
}

#[test]
fn invalid_price_string_leaves_none() {
    let event = make_event(r#"{
        "event_type": "last_trade_price",
        "asset_id": "tok1",
        "price": "not_a_number"
    }"#);
    let mut ts = TokenState::default();
    apply_ws_event(&event, &mut ts);

    assert_eq!(ts.last_trade_price, None);
}

#[test]
fn serde_ws_event_missing_optional_fields() {
    let event: WsEvent = serde_json::from_str(r#"{"event_type": "book"}"#).unwrap();
    assert_eq!(event.asset_id, None);
    assert_eq!(event.bids, None);
}
