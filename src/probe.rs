use std::collections::HashMap;
use std::fmt::Write;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::Query;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use tokio::sync::mpsc;
use tracing::{error, info};

use crate::api::clob::ClobClient;
use crate::api::data::DataClient;
use crate::api::gamma::GammaClient;
use crate::state::{SlugRegistry, TokenState};
use crate::ws::market::WsCommand;

#[derive(Debug, serde::Deserialize)]
pub struct ProbeParams {
    pub target: Option<String>,
}

pub struct ProbeState {
    pub registry: Arc<SlugRegistry>,
    pub gamma: Arc<GammaClient>,
    pub clob: Arc<ClobClient>,
    pub data: Arc<DataClient>,
    pub ws_cmd_tx: mpsc::Sender<WsCommand>,
}

pub async fn handle_probe(
    Query(params): Query<ProbeParams>,
    state: axum::extract::State<Arc<ProbeState>>,
) -> impl IntoResponse {
    let slug = match params.target {
        Some(s) if !s.is_empty() => s,
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                "missing ?target= query parameter".to_string(),
            );
        }
    };

    if let Err(e) = ensure_active(&slug, &state).await {
        error!("failed to activate slug '{}': {:#}", slug, e);
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to activate slug '{}': {}", slug, e),
        );
    }

    if let Err(e) = refresh_rest_data(&slug, &state).await {
        error!("failed to refresh REST data for '{}': {:#}", slug, e);
    }

    let body = render_metrics(&slug, &state.registry).await;
    (StatusCode::OK, body)
}

async fn ensure_active(slug: &str, state: &ProbeState) -> anyhow::Result<()> {
    if state.registry.is_active(slug).await {
        return Ok(());
    }

    info!("activating slug '{}'", slug);

    let resolved = state.gamma.resolve_slug(slug).await?;

    let mut initial_states: HashMap<String, TokenState> = HashMap::new();

    for market in &resolved.markets {
        for ti in market.token_infos() {
            let mut ts = TokenState::default();

            match state.clob.get_book(&ti.token_id).await {
                Ok(book) => {
                    ts.min_order_size = book
                        .min_order_size
                        .as_deref()
                        .and_then(|s| s.parse().ok());
                    ts.tick_size = book.tick_size.as_deref().and_then(|s| s.parse().ok());
                    ts.last_trade_price = book
                        .last_trade_price
                        .as_deref()
                        .and_then(|s| s.parse().ok());
                    ts.best_bid = book
                        .bids
                        .iter()
                        .filter_map(|b| b.price.parse::<f64>().ok())
                        .reduce(f64::max);
                    ts.best_ask = book
                        .asks
                        .iter()
                        .filter_map(|a| a.price.parse::<f64>().ok())
                        .reduce(f64::min);
                    if let (Some(bid), Some(ask)) = (ts.best_bid, ts.best_ask) {
                        ts.spread = Some(ask - bid);
                    }
                }
                Err(e) => {
                    error!("failed to fetch book for token {}: {:#}", ti.token_id, e);
                }
            }

            match state.clob.get_fee_rate(&ti.token_id).await {
                Ok(fr) => {
                    ts.fee_rate_bps = Some(fr.base_fee);
                }
                Err(e) => {
                    error!(
                        "failed to fetch fee rate for token {}: {:#}",
                        ti.token_id, e
                    );
                }
            }

            initial_states.insert(ti.token_id.clone(), ts);
        }
    }

    let token_ids = state.registry.activate(slug, &resolved, initial_states).await;

    if !token_ids.is_empty() {
        let _ = state.ws_cmd_tx.send(WsCommand::Subscribe(token_ids)).await;
    }

    Ok(())
}

/// Refresh REST-polled data (holders, OI) if their cache TTLs have expired.
async fn refresh_rest_data(slug: &str, state: &ProbeState) -> anyhow::Result<()> {
    let cache_cfg = &state.registry.cache_config;
    let oi_ttl = Duration::from_secs(cache_cfg.open_interest_ttl_secs);
    let holders_ttl = Duration::from_secs(cache_cfg.holders_ttl_secs);

    let (need_oi, need_holders) = {
        let slugs = state.registry.slugs.read().await;
        let ss = slugs.get(slug);
        match ss {
            Some(s) => (s.is_oi_expired(oi_ttl), s.is_holders_expired(holders_ttl)),
            None => return Ok(()),
        }
    };

    let condition_ids = state.registry.condition_ids_for_slug(slug).await;
    let cid_refs: Vec<&str> = condition_ids.iter().map(|s| s.as_str()).collect();

    if need_oi && !cid_refs.is_empty() {
        match state.data.get_open_interest(&cid_refs).await {
            Ok(oi_data) => {
                let mut slugs = state.registry.slugs.write().await;
                if let Some(ss) = slugs.get_mut(slug) {
                    ss.set_open_interest(oi_data);
                }
            }
            Err(e) => error!("failed to fetch OI for '{}': {:#}", slug, e),
        }
    }

    if need_holders && !cid_refs.is_empty() {
        match state.data.get_top_holders(&cid_refs).await {
            Ok(holders_data) => {
                let mut slugs = state.registry.slugs.write().await;
                if let Some(ss) = slugs.get_mut(slug) {
                    ss.set_top_holders(holders_data);
                }
            }
            Err(e) => error!("failed to fetch holders for '{}': {:#}", slug, e),
        }
    }

    Ok(())
}

async fn render_metrics(slug: &str, registry: &SlugRegistry) -> String {
    let slugs = registry.slugs.read().await;
    let ss = match slugs.get(slug) {
        Some(s) => s,
        None => return String::new(),
    };

    let mut out = String::with_capacity(4096);

    write_help_type(&mut out, "polymarket_spread_bid", "Bid price closest to the spread", "gauge");
    write_help_type(&mut out, "polymarket_spread_ask", "Ask price closest to the spread", "gauge");
    write_help_type(&mut out, "polymarket_spread", "Bid-ask spread", "gauge");
    write_help_type(
        &mut out,
        "polymarket_last_trade_price",
        "Last trade price",
        "gauge",
    );
    write_help_type(
        &mut out,
        "polymarket_tick_size",
        "Minimum tick size",
        "gauge",
    );
    write_help_type(
        &mut out,
        "polymarket_min_order_size",
        "Minimum order size",
        "gauge",
    );
    write_help_type(
        &mut out,
        "polymarket_fee_rate_bps",
        "Fee rate in basis points",
        "gauge",
    );
    write_help_type(
        &mut out,
        "polymarket_open_interest",
        "Open interest value",
        "gauge",
    );
    write_help_type(
        &mut out,
        "polymarket_top_holder_amount",
        "Top holder position amount",
        "gauge",
    );

    for market in &ss.markets {
        let q = &market.question;
        let cid = &market.condition_id;

        for ti in &market.tokens {
            let tid = &ti.token_id;
            let outcome = &ti.outcome;

            let ts = match ss.tokens.get(tid) {
                Some(t) => t,
                None => continue,
            };

            let labels = format!(
                "question=\"{}\",outcome=\"{}\",token_id=\"{}\"",
                escape_label(q),
                escape_label(outcome),
                escape_label(tid),
            );

            if let Some(v) = ts.best_bid {
                let _ = writeln!(out, "polymarket_spread_bid{{{}}} {}", labels, v);
            }
            if let Some(v) = ts.best_ask {
                let _ = writeln!(out, "polymarket_spread_ask{{{}}} {}", labels, v);
            }
            if let Some(v) = ts.spread {
                let _ = writeln!(out, "polymarket_spread{{{}}} {}", labels, v);
            }
            if let Some(v) = ts.last_trade_price {
                let _ = writeln!(out, "polymarket_last_trade_price{{{}}} {}", labels, v);
            }
            if let Some(v) = ts.tick_size {
                let _ = writeln!(out, "polymarket_tick_size{{{}}} {}", labels, v);
            }
            if let Some(v) = ts.min_order_size {
                let _ = writeln!(out, "polymarket_min_order_size{{{}}} {}", labels, v);
            }
            if let Some(v) = ts.fee_rate_bps {
                let _ = writeln!(out, "polymarket_fee_rate_bps{{{}}} {}", labels, v);
            }

            // Top holders for this token
            if let Some(holders) = ss.get_top_holders(tid) {
                for (rank, holder) in holders.iter().enumerate() {
                    let amount = holder.amount.unwrap_or(0.0);
                    let name = holder.display_name();
                    let holder_labels = format!(
                        "question=\"{}\",outcome=\"{}\",holder_name=\"{}\",holder_rank=\"{}\"",
                        escape_label(q),
                        escape_label(outcome),
                        escape_label(&name),
                        rank + 1,
                    );
                    let _ = writeln!(
                        out,
                        "polymarket_top_holder_amount{{{}}} {}",
                        holder_labels, amount
                    );
                }
            }
        }

        // Open interest per condition_id (market-level, not per-token)
        if let Some(oi) = ss.get_open_interest(cid) {
            let oi_labels = format!(
                "question=\"{}\",condition_id=\"{}\"",
                escape_label(q),
                escape_label(cid),
            );
            let _ = writeln!(out, "polymarket_open_interest{{{}}} {}", oi_labels, oi);
        }
    }

    out
}

fn write_help_type(out: &mut String, name: &str, help: &str, metric_type: &str) {
    let _ = writeln!(out, "# HELP {} {}", name, help);
    let _ = writeln!(out, "# TYPE {} {}", name, metric_type);
}

fn escape_label(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

#[cfg(test)]
#[path = "tests/probe.rs"]
mod tests;
