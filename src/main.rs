mod api;
mod config;
mod probe;
mod rate_limiter;
mod state;
mod ws;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use clap::Parser;
use tokio::sync::mpsc;
use tracing::{info, warn};

use crate::api::clob::ClobClient;
use crate::api::data::DataClient;
use crate::api::gamma::GammaClient;
use crate::config::Config;
use crate::probe::{handle_probe, ProbeState};
use crate::rate_limiter::RateLimiterService;
use crate::state::SlugRegistry;

#[derive(Parser)]
#[command(name = "polymarket-exporter", about = "Prometheus exporter for Polymarket")]
struct Cli {
    /// Path to config file
    #[arg(short, long, default_value = "config.toml", env = "CONFIG_PATH")]
    config: PathBuf,

    /// Override listen address
    #[arg(short, long, env = "LISTEN_ADDR")]
    listen: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "polymarket_exporter=info,warn".into()),
        )
        .init();

    let cli = Cli::parse();

    let cfg = if cli.config.exists() {
        Config::load(&cli.config)?
    } else {
        warn!(
            "config file '{}' not found, using defaults",
            cli.config.display()
        );
        Config::default()
    };

    let listen_addr: SocketAddr = cli
        .listen
        .as_deref()
        .unwrap_or(&cfg.server.listen_addr)
        .parse()?;

    let rate_limiter = Arc::new(RateLimiterService::from_config(&cfg.rate_limits));
    let http_client = reqwest::Client::new();

    let gamma = Arc::new(GammaClient::new(http_client.clone(), rate_limiter.clone()));
    let clob = Arc::new(ClobClient::new(http_client.clone(), rate_limiter.clone()));
    let data = Arc::new(DataClient::new(http_client.clone(), rate_limiter.clone()));

    let registry = Arc::new(SlugRegistry::new(cfg.cache));
    let ws_connected = Arc::new(AtomicBool::new(false));

    let (ws_cmd_tx, ws_cmd_rx) = mpsc::channel::<ws::market::WsCommand>(256);

    let probe_state = Arc::new(ProbeState {
        registry: registry.clone(),
        gamma,
        clob,
        data,
        ws_cmd_tx,
    });

    let ws_registry = registry.clone();
    let ws_connected_flag = ws_connected.clone();
    tokio::spawn(async move {
        ws::market::run_ws_manager(ws_registry, ws_cmd_rx, ws_connected_flag).await;
    });

    let metrics_state = (registry.clone(), ws_connected.clone());
    let app = Router::new()
        .route("/probe", get(handle_probe))
        .route(
            "/metrics",
            get(move || {
                let (reg, ws) = metrics_state.clone();
                async move { handle_internal_metrics(reg, ws).await }
            }),
        )
        .with_state(probe_state);

    info!("listening on {}", listen_addr);

    let listener = tokio::net::TcpListener::bind(listen_addr).await?;

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    info!("shutting down");
    Ok(())
}

async fn handle_internal_metrics(
    registry: Arc<SlugRegistry>,
    ws_connected: Arc<AtomicBool>,
) -> impl IntoResponse {
    let active_slugs = registry.slugs.read().await.len();
    let ws = if ws_connected.load(Ordering::Relaxed) {
        1
    } else {
        0
    };

    let body = format!(
        "# HELP polymarket_exporter_active_slugs Number of actively tracked slugs\n\
         # TYPE polymarket_exporter_active_slugs gauge\n\
         polymarket_exporter_active_slugs {}\n\
         # HELP polymarket_exporter_websocket_connected Whether the WebSocket is connected\n\
         # TYPE polymarket_exporter_websocket_connected gauge\n\
         polymarket_exporter_websocket_connected {}\n",
        active_slugs, ws,
    );

    (StatusCode::OK, body)
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to listen for ctrl+c");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to listen for SIGTERM")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    info!("received shutdown signal");
}
