use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::json;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::time::{interval, timeout};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};
use tracing::{debug, error, info, warn};

use crate::state::SlugRegistry;

const WS_URL: &str = "wss://ws-subscriptions-clob.polymarket.com/ws/market";
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(10);
const RECONNECT_DELAY: Duration = Duration::from_secs(5);
const READ_TIMEOUT: Duration = Duration::from_secs(30);

pub enum WsCommand {
    Subscribe(Vec<String>),
}

#[derive(Debug, Deserialize)]
struct WsEvent {
    event_type: Option<String>,
    asset_id: Option<String>,
    #[allow(dead_code)]
    market: Option<String>,

    // book
    bids: Option<Vec<BookLevel>>,
    asks: Option<Vec<BookLevel>>,

    // best_bid_ask
    best_bid: Option<String>,
    best_ask: Option<String>,
    spread: Option<String>,

    // last_trade_price
    price: Option<String>,

    // tick_size_change
    new_tick_size: Option<String>,
}

#[derive(Debug, Deserialize, PartialEq)]
struct BookLevel {
    price: String,
    #[allow(dead_code)]
    size: String,
}

/// Long-running WebSocket manager task.
/// Waits for the first Subscribe command before connecting, then keeps
/// the connection alive, dispatching events to `registry`.
pub async fn run_ws_manager(
    registry: Arc<SlugRegistry>,
    mut cmd_rx: mpsc::Receiver<WsCommand>,
    ws_connected: Arc<AtomicBool>,
) {
    let mut pending_subscribes: Vec<String> = Vec::new();

    // Wait for at least one subscription before connecting.
    info!("WebSocket manager waiting for first subscription...");
    loop {
        match cmd_rx.recv().await {
            Some(WsCommand::Subscribe(ids)) => {
                pending_subscribes.extend(ids);
                break;
            }
            None => {
                info!("WebSocket command channel closed, shutting down");
                return;
            }
        }
    }
    info!(
        "first subscription received ({} tokens), connecting",
        pending_subscribes.len()
    );

    loop {
        info!("connecting to Polymarket WebSocket...");

        match connect_async(WS_URL).await {
            Ok((ws_stream, _)) => {
                info!("WebSocket connected");
                ws_connected.store(true, Ordering::Relaxed);
                if let Err(e) = handle_connection(
                    ws_stream,
                    &registry,
                    &mut cmd_rx,
                    &mut pending_subscribes,
                )
                .await
                {
                    warn!("WebSocket session ended: {:#}", e);
                }
                ws_connected.store(false, Ordering::Relaxed);
            }
            Err(e) => {
                error!("failed to connect WebSocket: {:#}", e);
            }
        }

        info!("reconnecting in {:?}...", RECONNECT_DELAY);
        tokio::time::sleep(RECONNECT_DELAY).await;
    }
}

async fn handle_connection(
    ws_stream: WebSocketStream<MaybeTlsStream<TcpStream>>,
    registry: &Arc<SlugRegistry>,
    cmd_rx: &mut mpsc::Receiver<WsCommand>,
    pending_subscribes: &mut Vec<String>,
) -> Result<()> {
    let (mut sink, mut stream) = ws_stream.split();

    // Collect all token IDs we need to subscribe to (existing + pending).
    let mut all_ids = registry.all_token_ids().await;
    all_ids.extend(pending_subscribes.drain(..));
    all_ids.sort();
    all_ids.dedup();

    if !all_ids.is_empty() {
        let sub_msg = json!({
            "assets_ids": all_ids,
            "type": "market",
            "custom_feature_enabled": true,
        });
        sink.send(Message::Text(sub_msg.to_string().into()))
            .await?;
        info!("subscribed to {} token IDs", all_ids.len());
    }

    let mut heartbeat = interval(HEARTBEAT_INTERVAL);

    loop {
        tokio::select! {
            _ = heartbeat.tick() => {
                sink.send(Message::Text("PING".into())).await?;
                debug!("sent PING");
            }

            Some(cmd) = cmd_rx.recv() => {
                match cmd {
                    WsCommand::Subscribe(ids) => {
                        let sub_msg = json!({
                            "assets_ids": ids,
                            "operation": "subscribe",
                            "custom_feature_enabled": true,
                        });
                        if let Err(e) = sink.send(Message::Text(sub_msg.to_string().into())).await {
                            warn!("failed to send subscribe, queuing for reconnect: {:#}", e);
                            pending_subscribes.extend(ids);
                            anyhow::bail!("WebSocket send failed");
                        }
                        info!("dynamic subscribe sent for {} tokens", ids.len());
                    }
                }
            }

            msg_result = timeout(READ_TIMEOUT, stream.next()) => {
                match msg_result {
                    Ok(Some(Ok(Message::Text(text)))) => {
                        let text_str: &str = &text;
                        if text_str == "PONG" {
                            debug!("received PONG");
                            continue;
                        }
                        if let Err(e) = dispatch_event(text_str, registry).await {
                            debug!("failed to dispatch event: {:#}", e);
                        }
                    }
                    Ok(Some(Ok(Message::Ping(data)))) => {
                        sink.send(Message::Pong(data)).await?;
                        debug!("responded to server Ping with Pong");
                    }
                    Ok(Some(Ok(Message::Close(_)))) => {
                        info!("WebSocket closed by server");
                        anyhow::bail!("server closed connection");
                    }
                    Ok(Some(Err(e))) => {
                        anyhow::bail!("WebSocket error: {:#}", e);
                    }
                    Ok(None) => {
                        anyhow::bail!("WebSocket stream ended");
                    }
                    Err(_) => {
                        warn!("no message received in {:?}, reconnecting", READ_TIMEOUT);
                        anyhow::bail!("read timeout");
                    }
                    Ok(Some(Ok(_))) => {
                        // Binary or other frame types — ignore
                    }
                }
            }
        }
    }
}

async fn dispatch_event(text: &str, registry: &SlugRegistry) -> Result<()> {
    let event: WsEvent =
        serde_json::from_str(text).map_err(|e| anyhow::anyhow!("parse error: {}", e))?;

    let asset_id = event.asset_id.as_deref().unwrap_or("");

    if asset_id.is_empty() {
        return Ok(());
    }

    let slug = {
        let t2s = registry.token_to_slug.read().await;
        t2s.get(asset_id).cloned()
    };

    let slug = match slug {
        Some(s) => s,
        None => return Ok(()),
    };

    let mut slugs = registry.slugs.write().await;
    let slug_state = match slugs.get_mut(&slug) {
        Some(s) => s,
        None => return Ok(()),
    };

    let token_state = slug_state
        .tokens
        .entry(asset_id.to_string())
        .or_default();

    apply_ws_event(&event, token_state);

    Ok(())
}

fn apply_ws_event(event: &WsEvent, token_state: &mut crate::state::TokenState) {
    let event_type = event.event_type.as_deref().unwrap_or("");
    match event_type {
        "book" => {
            if let Some(bids) = &event.bids {
                token_state.best_bid = bids
                    .iter()
                    .filter_map(|b| b.price.parse::<f64>().ok())
                    .reduce(f64::max);
            }
            if let Some(asks) = &event.asks {
                token_state.best_ask = asks
                    .iter()
                    .filter_map(|a| a.price.parse::<f64>().ok())
                    .reduce(f64::min);
            }
            if let (Some(bid), Some(ask)) = (token_state.best_bid, token_state.best_ask) {
                token_state.spread = Some(ask - bid);
            }
        }
        "best_bid_ask" => {
            if let Some(v) = &event.best_bid {
                token_state.best_bid = v.parse().ok();
            }
            if let Some(v) = &event.best_ask {
                token_state.best_ask = v.parse().ok();
            }
            if let Some(v) = &event.spread {
                token_state.spread = v.parse().ok();
            }
        }
        "last_trade_price" => {
            if let Some(v) = &event.price {
                token_state.last_trade_price = v.parse().ok();
            }
        }
        "tick_size_change" => {
            if let Some(v) = &event.new_tick_size {
                token_state.tick_size = v.parse().ok();
            }
        }
        _ => {}
    }
}

#[cfg(test)]
#[path = "../tests/market.rs"]
mod tests;
