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
use crate::api::data::{DataClient, Position};
use crate::api::gamma::{GammaClient, TokenInfo};
use crate::state::{SlugRegistry, TokenState};
use crate::ws::market::WsCommand;

#[derive(Debug, serde::Deserialize)]
pub struct ProbeParams {
    pub target: Option<String>,
    pub module: Option<String>,
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
    let target = match params.target {
        Some(s) if !s.is_empty() => s,
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                "missing ?target= query parameter".to_string(),
            );
        }
    };

    match params.module.as_deref().unwrap_or("market") {
        "market" => handle_market_probe(&target, &state).await,
        "wallet" => handle_wallet_probe(&target, &state).await,
        module => (
            StatusCode::BAD_REQUEST,
            format!("unknown module '{}'; expected 'market' or 'wallet'", module),
        ),
    }
}

async fn handle_market_probe(slug: &str, state: &ProbeState) -> (StatusCode, String) {
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

    let body = render_market_metrics(&slug, &state.registry).await;
    (StatusCode::OK, body)
}

async fn handle_wallet_probe(wallet: &str, state: &ProbeState) -> (StatusCode, String) {
    if !is_wallet_address(wallet) {
        return (
            StatusCode::BAD_REQUEST,
            "wallet target must be a 0x-prefixed, 40-hex-character address".to_string(),
        );
    }

    let result = async {
        let positions = state.data.get_positions(wallet).await?;
        let all_time_realized_pnl = state.data.get_all_time_realized_pnl(wallet).await?;
        Ok::<_, anyhow::Error>((positions, all_time_realized_pnl))
    }
    .await;

    match result {
        Ok((positions, all_time_realized_pnl)) => (
            StatusCode::OK,
            render_wallet_metrics(wallet, &positions, all_time_realized_pnl),
        ),
        Err(e) => {
            error!("failed to fetch wallet data for '{}': {:#}", wallet, e);
            (
                StatusCode::BAD_GATEWAY,
                format!("failed to fetch wallet data for '{}': {}", wallet, e),
            )
        }
    }
}

fn is_wallet_address(wallet: &str) -> bool {
    wallet.len() == 42
        && wallet.starts_with("0x")
        && wallet.as_bytes()[2..].iter().all(u8::is_ascii_hexdigit)
}

async fn ensure_active(slug: &str, state: &ProbeState) -> anyhow::Result<()> {
    if state.registry.is_active(slug).await {
        return Ok(());
    }

    info!("activating slug '{}'", slug);

    let resolved = state.gamma.resolve_slug(slug).await?;

    // Fetch authoritative token-outcome mappings from CLOB API.
    // Falls back to Gamma's mapping if CLOB fetch fails.
    let mut clob_tokens: HashMap<String, Vec<TokenInfo>> = HashMap::new();
    for market in &resolved.markets {
        match state.clob.get_market(&market.condition_id).await {
            Ok(cm) => {
                let tokens: Vec<TokenInfo> = cm
                    .tokens
                    .iter()
                    .map(|t| TokenInfo {
                        token_id: t.token_id.clone(),
                        outcome: t.outcome.clone(),
                    })
                    .collect();
                clob_tokens.insert(market.condition_id.clone(), tokens);
            }
            Err(e) => {
                error!(
                    "failed to fetch CLOB market for {}, falling back to Gamma: {:#}",
                    market.condition_id, e
                );
            }
        }
    }

    let mut initial_states: HashMap<String, TokenState> = HashMap::new();

    for market in &resolved.markets {
        let token_infos = clob_tokens
            .get(&market.condition_id)
            .cloned()
            .unwrap_or_else(|| market.token_infos());

        for ti in &token_infos {
            let mut ts = TokenState::default();

            match state.clob.get_book(&ti.token_id).await {
                Ok(book) => {
                    ts.min_order_size = book.min_order_size.as_deref().and_then(|s| s.parse().ok());
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

    let token_ids = state
        .registry
        .activate(slug, &resolved, &clob_tokens, initial_states)
        .await;

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

async fn render_market_metrics(slug: &str, registry: &SlugRegistry) -> String {
    let slugs = registry.slugs.read().await;
    let ss = match slugs.get(slug) {
        Some(s) => s,
        None => return String::new(),
    };

    let mut out = String::with_capacity(4096);

    write_help_type(
        &mut out,
        "polymarket_market_spread_bid",
        "Bid price closest to the spread",
        "gauge",
    );
    write_help_type(
        &mut out,
        "polymarket_market_spread_ask",
        "Ask price closest to the spread",
        "gauge",
    );
    write_help_type(
        &mut out,
        "polymarket_market_spread",
        "Bid-ask spread",
        "gauge",
    );
    write_help_type(
        &mut out,
        "polymarket_market_last_trade_price",
        "Last trade price",
        "gauge",
    );
    write_help_type(
        &mut out,
        "polymarket_market_tick_size",
        "Minimum tick size",
        "gauge",
    );
    write_help_type(
        &mut out,
        "polymarket_market_min_order_size",
        "Minimum order size",
        "gauge",
    );
    write_help_type(
        &mut out,
        "polymarket_market_fee_rate_bps",
        "Fee rate in basis points",
        "gauge",
    );
    write_help_type(
        &mut out,
        "polymarket_market_open_interest",
        "Open interest value",
        "gauge",
    );
    write_help_type(
        &mut out,
        "polymarket_market_top_holder_amount",
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
                let _ = writeln!(out, "polymarket_market_spread_bid{{{}}} {}", labels, v);
            }
            if let Some(v) = ts.best_ask {
                let _ = writeln!(out, "polymarket_market_spread_ask{{{}}} {}", labels, v);
            }
            if let Some(v) = ts.spread {
                let _ = writeln!(out, "polymarket_market_spread{{{}}} {}", labels, v);
            }
            if let Some(v) = ts.last_trade_price {
                let _ = writeln!(
                    out,
                    "polymarket_market_last_trade_price{{{}}} {}",
                    labels, v
                );
            }
            if let Some(v) = ts.tick_size {
                let _ = writeln!(out, "polymarket_market_tick_size{{{}}} {}", labels, v);
            }
            if let Some(v) = ts.min_order_size {
                let _ = writeln!(out, "polymarket_market_min_order_size{{{}}} {}", labels, v);
            }
            if let Some(v) = ts.fee_rate_bps {
                let _ = writeln!(out, "polymarket_market_fee_rate_bps{{{}}} {}", labels, v);
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
                        "polymarket_market_top_holder_amount{{{}}} {}",
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
            let _ = writeln!(
                out,
                "polymarket_market_open_interest{{{}}} {}",
                oi_labels, oi
            );
        }
    }

    out
}

fn render_wallet_metrics(
    wallet: &str,
    positions: &[Position],
    all_time_realized_pnl: Option<f64>,
) -> String {
    let mut out = String::with_capacity(4096);

    let metrics = [
        ("polymarket_wallet_position_shares", "Open position shares"),
        (
            "polymarket_wallet_position_average_price",
            "Average price paid per position share in USDC",
        ),
        (
            "polymarket_wallet_position_current_price",
            "Current price per position share in USDC",
        ),
        (
            "polymarket_wallet_position_traded_usdc",
            "USDC paid for the current open position",
        ),
        (
            "polymarket_wallet_position_payout_if_win_usdc",
            "USDC paid if all current position shares settle at 1",
        ),
        (
            "polymarket_wallet_position_current_value_usdc",
            "Current position value in USDC",
        ),
        (
            "polymarket_wallet_position_unrealized_gain_usdc",
            "Unrealized gain or loss from current value minus traded amount",
        ),
        (
            "polymarket_wallet_position_unrealized_gain_ratio",
            "Unrealized gain or loss as a ratio of traded amount",
        ),
        (
            "polymarket_wallet_position_cash_pnl_usdc",
            "Position cash profit or loss in USDC, as reported by Polymarket",
        ),
        (
            "polymarket_wallet_position_pnl_ratio",
            "Position profit or loss as a ratio of initial value",
        ),
        (
            "polymarket_wallet_position_realized_pnl_usdc",
            "Realized position profit or loss in USDC",
        ),
        (
            "polymarket_wallet_all_time_realized_pnl_usdc",
            "All-time realized profit or loss in USDC, including closed position history",
        ),
    ];
    for (name, help) in metrics {
        write_help_type(&mut out, name, help, "gauge");
    }

    for position in positions {
        if position.redeemable {
            continue;
        }

        let labels = format!(
            "wallet=\"{}\",condition_id=\"{}\",token_id=\"{}\",question=\"{}\",outcome=\"{}\"",
            escape_label(wallet),
            escape_label(&position.condition_id),
            escape_label(&position.asset),
            escape_label(&position.title),
            escape_label(&position.outcome),
        );
        let unrealized_gain = position.current_value - position.initial_value;
        let unrealized_gain_ratio = if position.initial_value == 0.0 {
            f64::NAN
        } else {
            unrealized_gain / position.initial_value
        };
        let _ = writeln!(
            out,
            "polymarket_wallet_position_shares{{{}}} {}",
            labels, position.size
        );
        let _ = writeln!(
            out,
            "polymarket_wallet_position_average_price{{{}}} {}",
            labels, position.avg_price
        );
        let _ = writeln!(
            out,
            "polymarket_wallet_position_current_price{{{}}} {}",
            labels, position.cur_price
        );
        let _ = writeln!(
            out,
            "polymarket_wallet_position_traded_usdc{{{}}} {}",
            labels, position.initial_value
        );
        let _ = writeln!(
            out,
            "polymarket_wallet_position_payout_if_win_usdc{{{}}} {}",
            labels, position.size
        );
        let _ = writeln!(
            out,
            "polymarket_wallet_position_current_value_usdc{{{}}} {}",
            labels, position.current_value
        );
        let _ = writeln!(
            out,
            "polymarket_wallet_position_unrealized_gain_usdc{{{}}} {}",
            labels, unrealized_gain
        );
        let _ = writeln!(
            out,
            "polymarket_wallet_position_unrealized_gain_ratio{{{}}} {}",
            labels, unrealized_gain_ratio
        );
        let _ = writeln!(
            out,
            "polymarket_wallet_position_cash_pnl_usdc{{{}}} {}",
            labels, position.cash_pnl
        );
        let _ = writeln!(
            out,
            "polymarket_wallet_position_pnl_ratio{{{}}} {}",
            labels,
            position.percent_pnl / 100.0
        );
        let _ = writeln!(
            out,
            "polymarket_wallet_position_realized_pnl_usdc{{{}}} {}",
            labels, position.realized_pnl
        );
    }

    if let Some(realized_pnl) = all_time_realized_pnl {
        let _ = writeln!(
            out,
            "polymarket_wallet_all_time_realized_pnl_usdc{{wallet=\"{}\"}} {}",
            escape_label(wallet),
            realized_pnl
        );
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
