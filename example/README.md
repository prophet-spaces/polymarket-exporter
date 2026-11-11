# Polymarket Monitoring Stack

Full Prometheus + Grafana + Alertmanager stack for monitoring Polymarket prediction markets.

```
┌──────────┐    scrape     ┌─────────────────────┐    probe     ┌──────────────┐
│Prometheus├───────────────►│polymarket-exporter  ├────────────►│ Polymarket   │
│  :9090   │               │       :9184         │  (REST+WS)  │    APIs      │
└────┬─────┘               └─────────────────────┘             └──────────────┘
     │
     ├──── alerts ────►  Alertmanager :9093  ────►  Telegram Bot
     │
     └──── query  ────►  Grafana :3000
```

## Prerequisites

- Docker and Docker Compose

## Quick Start

```bash
cd example

# (Optional) Set up Telegram alerts — see below
cp .env.example .env
# Edit .env with your bot token and chat ID

docker compose pull polymarket-exporter
docker compose up -d
```

## Services

| Service              | URL                                |
|----------------------|------------------------------------|
| Grafana              | http://localhost:3000 (admin/admin) |
| Prometheus           | http://localhost:9090               |
| Alertmanager         | http://localhost:9093               |
| Polymarket Exporter  | http://localhost:9184               |

Grafana default login: `admin` / `admin`. Anonymous read access is enabled.

## Monitored Markets

The stack scrapes two example markets (configured in `prometheus/prometheus.yml`):

- `will-jesus-christ-return-before-2027`
- `will-china-invade-taiwan-by-december-31-2027`

To add more markets, append slugs to the `targets` list:

```yaml
static_configs:
  - targets:
      - will-jesus-christ-return-before-2027
      - will-china-invade-taiwan-by-december-31-2027
      - your-new-slug-here
```

## Monitored Wallet

The example scrapes one wallet every five minutes. Replace the zero address in
`prometheus/prometheus.yml` with your own `0x` address before using it:

```yaml
  - job_name: polymarket-wallet
    static_configs:
      - targets:
          - 0xyour-wallet-address
```

## Telegram Alerts Setup

1. Create a bot via [@BotFather](https://t.me/BotFather) on Telegram — save the bot token.
2. Get your chat ID by messaging [@userinfobot](https://t.me/userinfobot) or adding the bot to a group.
3. Create a `.env` file:

```env
TELEGRAM_BOT_TOKEN=123456789:ABCdefGhIjKlMnOpQrStUvWxYz
TELEGRAM_CHAT_ID=-1001234567890
```

4. Restart the stack: `docker compose up -d`

## Alert Rules

All polymarket metrics are **gauges** (not counters), so `rate()` cannot be used.
The rules use gauge-appropriate PromQL functions:

| Alert                             | Function    | Expression                                                            | Why                                           |
|-----------------------------------|-------------|-----------------------------------------------------------------------|-----------------------------------------------|
| MarketLargePriceMovement          | `delta()`   | `abs(delta(polymarket_market_last_trade_price[15m])) > 0.05`          | Catches sudden price jumps/drops              |
| MarketWideSpread                  | (direct)    | `(polymarket_market_spread / polymarket_market_tick_size) > 2` for 5m | Detects illiquid conditions (tick-normalised) |
| MarketLargeOpenInterestChange     | `delta()`   | `abs(delta(polymarket_market_open_interest[15m])) > 100000`           | Detects large money flows                     |
| MarketHighVolatility              | `changes()` | `changes(polymarket_market_last_trade_price[5m]) > 20` for 2m         | Detects unusually frequent price changes      |
| MarketSustainedPriceDrift         | `deriv()`   | `abs(deriv(polymarket_market_last_trade_price[10m])) > 0.001` for 5m  | Detects steady directional movement           |
| WalletLargePositionPriceMovement  | `delta()`   | `abs(delta(polymarket_wallet_position_current_price[15m])) > 0.05`    | Catches material position price moves         |
| WalletLargePositionCashLoss       | `delta()`   | `-delta(polymarket_wallet_position_cash_pnl_usdc[5m]) > 100`         | Catches a cash loss above 100 USDC in 5m      |
| WalletLargePositionPercentageLoss | `delta()`   | `-delta(polymarket_wallet_position_pnl_ratio[5m]) * 100 > 10`        | Catches a P&L drop above 10 points in 5m      |
| WalletProbeFailing                | `up`        | `up{job="polymarket-wallet"} == 0` for 2m                             | Wallet scrape health check                    |
| ExporterMarketProbeFailing        | `up`        | `up{job="polymarket-market"} == 0` for 2m                             | Market-probe health check                     |
| ExporterWebSocketDisconnected     | (direct)    | `websocket_connected == 0` for 5m                                     | Real-time feed health check                   |

## Test Alertmanager

Test you can get message

```bash
curl -X POST -H "Content-Type: application/json" \
-d '[{
  "labels": {
    "alertname": "TestAlert",
    "severity": "critical",
    "instance": "localhost"
  },
  "annotations": {
    "summary": "This is a manual test alert",
    "description": "Testing Alertmanager connectivity and receivers."
  }
}]' \
http://localhost:9093/api/v2/alerts
```
