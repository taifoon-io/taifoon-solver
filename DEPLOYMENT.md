# Deployment Guide - Taifoon Solver

## Production-Ready Deployment

This guide covers deploying the Taifoon Solver system to production with T3RN LWC integration.

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────┐
│                    TAIFOON SOLVER SYSTEM                     │
├─────────────────────────────────────────────────────────────┤
│                                                               │
│  ┌──────────────────┐      ┌──────────────────┐             │
│  │  Genome Stream   │─────▶│  Solver Backend  │             │
│  │  (SSE Client)    │      │  (Rust + Axum)   │             │
│  └──────────────────┘      └────────┬─────────┘             │
│                                      │                       │
│                                      │  SSE Events           │
│                                      ▼                       │
│                            ┌─────────────────┐               │
│                            │   Dashboard     │               │
│                            │   (Next.js 15)  │               │
│                            └─────────────────┘               │
│                                                               │
│  ┌──────────────────┐      ┌──────────────────┐             │
│  │ Profit Calculator│─────▶│    Executor      │             │
│  │  (Protocol Fees) │      │  (Liquidity Mgr) │             │
│  └──────────────────┘      └────────┬─────────┘             │
│                                      │                       │
│                                      ▼                       │
│                            ┌─────────────────┐               │
│                            │  T3RN Sidecar   │               │
│                            │  (LWC Orders)   │               │
│                            └─────────────────┘               │
│                                                               │
└─────────────────────────────────────────────────────────────┘
```

## System Requirements

### Hardware
- **CPU**: 2+ cores (4+ recommended for high throughput)
- **RAM**: 4GB minimum, 8GB recommended
- **Disk**: 10GB for binaries + logs
- **Network**: Stable connection with <100ms latency to RPC endpoints

### Software
- **Rust**: 1.75+ (for building from source)
- **Node.js**: 18+ (for dashboard)
- **OS**: Linux (Ubuntu 22.04+), macOS, or Docker

## Environment Configuration

### Required Environment Variables

Create a `.env` file in the project root:

```bash
# Solver Configuration
MIN_PROFIT_USD=0.10              # Minimum profit threshold ($0.10 default)
SIMULATION_MODE=false            # Set to false for live trading
SOLVER_INTEL_PATH=config/solver_intel.json

# T3RN LiquidityWellCompact Integration
T3RN_LWC_ENABLED=true            # Enable T3RN sidecar
WALLET_PRIVATE_KEY=0x...         # Your wallet private key (KEEP SECRET!)

# API Configuration
API_PORT=8082                    # Solver API port (default: 8082)
GENOME_SSE_URL=https://api.taifoon.dev/api/genome/subscribe/sse

# Dashboard Configuration
NEXT_PUBLIC_SOLVER_API=http://localhost:8082

# Logging (optional)
RUST_LOG=info                    # debug, info, warn, error
```

### Security Best Practices

⚠️ **NEVER commit `.env` files to git!**

```bash
# Add to .gitignore
echo ".env" >> .gitignore
echo ".env.*" >> .gitignore
```

**Private Key Management:**
- Use hardware wallets in production (e.g., Ledger integration)
- For software wallets, encrypt the private key at rest
- Rotate keys periodically
- Use separate wallets for testing vs production
- Never share private keys across environments

## Build & Deploy

### Option 1: Native Binary (Recommended for Production)

```bash
# 1. Clone repository
git clone https://github.com/yawningmonsoon/taifoon-solver.git
cd taifoon-solver

# 2. Build release binary
cargo build --release

# 3. Binary located at:
./target/release/taifoon-solver

# 4. Copy to /usr/local/bin (optional)
sudo cp ./target/release/taifoon-solver /usr/local/bin/

# 5. Run with environment variables
export T3RN_LWC_ENABLED=true
export WALLET_PRIVATE_KEY=0x...
export SIMULATION_MODE=false
taifoon-solver
```

### Option 2: Docker Deployment

Create `Dockerfile`:

```dockerfile
FROM rust:1.75 as builder

WORKDIR /app
COPY . .

RUN cargo build --release

FROM ubuntu:22.04

RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/taifoon-solver /usr/local/bin/
COPY --from=builder /app/config /app/config

WORKDIR /app

EXPOSE 8082

CMD ["taifoon-solver"]
```

Build and run:

```bash
# Build image
docker build -t taifoon-solver:latest .

# Run container
docker run -d \
  --name taifoon-solver \
  --restart unless-stopped \
  -e T3RN_LWC_ENABLED=true \
  -e WALLET_PRIVATE_KEY=$WALLET_PRIVATE_KEY \
  -e SIMULATION_MODE=false \
  -e MIN_PROFIT_USD=0.10 \
  -p 8082:8082 \
  taifoon-solver:latest

# View logs
docker logs -f taifoon-solver
```

### Option 3: Systemd Service (Linux)

Create `/etc/systemd/system/taifoon-solver.service`:

```ini
[Unit]
Description=Taifoon Solver - Cross-chain Intent Executor
After=network.target

[Service]
Type=simple
User=solver
Group=solver
WorkingDirectory=/opt/taifoon-solver
ExecStart=/usr/local/bin/taifoon-solver

# Environment
Environment="T3RN_LWC_ENABLED=true"
Environment="SIMULATION_MODE=false"
Environment="MIN_PROFIT_USD=0.10"
EnvironmentFile=/opt/taifoon-solver/.env

# Security
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=/opt/taifoon-solver/logs

# Restart policy
Restart=on-failure
RestartSec=10s

[Install]
WantedBy=multi-user.target
```

Enable and start:

```bash
# Create solver user
sudo useradd -r -s /bin/false solver

# Setup directories
sudo mkdir -p /opt/taifoon-solver/{logs,config}
sudo chown -R solver:solver /opt/taifoon-solver

# Copy binary and config
sudo cp target/release/taifoon-solver /usr/local/bin/
sudo cp config/* /opt/taifoon-solver/config/

# Start service
sudo systemctl daemon-reload
sudo systemctl enable taifoon-solver
sudo systemctl start taifoon-solver

# Check status
sudo systemctl status taifoon-solver
sudo journalctl -u taifoon-solver -f
```

## Dashboard Deployment

### Production Build

```bash
cd dashboard

# Install dependencies
npm install

# Build production bundle
npm run build

# Start production server
npm start
```

### Deploy to Vercel (Recommended)

```bash
# Install Vercel CLI
npm i -g vercel

# Deploy
cd dashboard
vercel --prod

# Set environment variable in Vercel dashboard
# NEXT_PUBLIC_SOLVER_API=https://solver.yourdomain.com
```

### Deploy to Nginx

```bash
# Build static export
cd dashboard
npm run build

# Copy to nginx directory
sudo cp -r out/* /var/www/taifoon-dashboard/

# Nginx config
sudo nano /etc/nginx/sites-available/taifoon-dashboard
```

Nginx configuration:

```nginx
server {
    listen 80;
    server_name dashboard.yourdomain.com;

    root /var/www/taifoon-dashboard;
    index index.html;

    location / {
        try_files $uri $uri/ /index.html;
    }

    # Proxy solver API
    location /api/solver {
        proxy_pass http://localhost:8082/api/solver;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
        proxy_set_header Host $host;
        proxy_cache_bypass $http_upgrade;
    }
}
```

Enable and reload:

```bash
sudo ln -s /etc/nginx/sites-available/taifoon-dashboard /etc/nginx/sites-enabled/
sudo nginx -t
sudo systemctl reload nginx
```

## Monitoring & Observability

### Health Checks

```bash
# Solver health
curl http://localhost:8082/api/solver/stats

# Dashboard health
curl http://localhost:3000/api/health  # If you add a health endpoint
```

### Prometheus Metrics (Future Enhancement)

Add to solver crate:

```rust
// crates/solver-main/Cargo.toml
prometheus = "0.13"

// Expose metrics endpoint
GET /metrics -> Prometheus format
```

### Log Aggregation

```bash
# Configure log rotation
sudo nano /etc/logrotate.d/taifoon-solver

/opt/taifoon-solver/logs/*.log {
    daily
    rotate 7
    compress
    delaycompress
    missingok
    notifempty
}
```

## Scaling Considerations

### Horizontal Scaling

The solver can be scaled horizontally with multiple instances:

```bash
# Run multiple instances with different MIN_PROFIT thresholds
# Instance 1: High-profit only
MIN_PROFIT_USD=1.0 taifoon-solver &

# Instance 2: Medium-profit
MIN_PROFIT_USD=0.50 taifoon-solver &

# Instance 3: Low-profit (aggressive)
MIN_PROFIT_USD=0.10 taifoon-solver &
```

**Note**: Ensure instances don't compete for the same intents. Use different profit thresholds or chain filters.

### Database Integration (Future)

For production at scale, add PostgreSQL for:
- Intent history
- Execution analytics
- Profit tracking
- Protocol performance

```sql
CREATE TABLE intents (
  id VARCHAR PRIMARY KEY,
  protocol VARCHAR NOT NULL,
  src_chain BIGINT NOT NULL,
  dst_chain BIGINT NOT NULL,
  amount VARCHAR NOT NULL,
  profit_usd NUMERIC(10,2),
  executed BOOLEAN DEFAULT false,
  tx_hash VARCHAR,
  created_at TIMESTAMP DEFAULT NOW()
);

CREATE INDEX idx_intents_protocol ON intents(protocol);
CREATE INDEX idx_intents_executed ON intents(executed);
CREATE INDEX idx_intents_created_at ON intents(created_at DESC);
```

## Troubleshooting

### Common Issues

**1. Genome stream disconnects**
```bash
# Check network connectivity
curl -N https://api.taifoon.dev/api/genome/subscribe/sse

# Increase reconnect delay in GenomeClient
```

**2. T3RN LWC orders failing**
```bash
# Verify wallet has ETH for gas
# Check LWC contract addresses in config.rs
# Review logs for specific error messages
```

**3. Dashboard not receiving events**
```bash
# Test SSE directly
curl -N http://localhost:8082/api/solver/stream

# Check CORS in solver API
# Verify NEXT_PUBLIC_SOLVER_API env var
```

**4. High memory usage**
```bash
# Reduce intent history retention
# Increase log rotation frequency
# Profile with valgrind or heaptrack
```

## Security Hardening

### Network Security

```bash
# Use firewall to restrict access
sudo ufw allow 8082/tcp  # Solver API
sudo ufw allow 3000/tcp  # Dashboard
sudo ufw enable

# For production, use HTTPS:
# - Setup Let's Encrypt SSL certificates
# - Configure nginx with TLS 1.3
# - Enable HSTS headers
```

### Rate Limiting

Add to Axum middleware:

```rust
use tower_governor::{governor::GovernorConfigBuilder, GovernorLayer};

let governor_conf = Box::new(
    GovernorConfigBuilder::default()
        .per_second(10)
        .burst_size(20)
        .finish()
        .unwrap(),
);

Router::new()
    .layer(GovernorLayer {
        config: Box::leak(governor_conf),
    })
```

## Maintenance

### Regular Tasks

**Daily:**
- Monitor profit performance
- Review execution success rate
- Check for failed transactions

**Weekly:**
- Update protocol registry
- Review gas cost trends
- Analyze liquidity source usage

**Monthly:**
- Security audit of wallet access
- Dependency updates (`cargo update`)
- Performance benchmarking

### Upgrades

```bash
# Zero-downtime deployment
# 1. Build new version
cargo build --release

# 2. Test in staging
SIMULATION_MODE=true ./target/release/taifoon-solver

# 3. Blue-green deployment
# Start new instance on different port
API_PORT=8083 ./target/release/taifoon-solver &

# 4. Switch traffic (nginx/load balancer)
# 5. Gracefully shutdown old instance
kill -SIGTERM <old_pid>
```

## Performance Benchmarks

### Target Metrics

- **Intent Detection Latency**: <100ms
- **Profitability Calc**: <50ms
- **Execution Decision**: <150ms total
- **SSE Event Propagation**: <200ms
- **API Response Time**: p99 <500ms
- **Uptime**: 99.9%

### Load Testing

```bash
# Install hey
go install github.com/rakyll/hey@latest

# Test API throughput
hey -n 10000 -c 100 http://localhost:8082/api/solver/stats

# Test SSE connections
for i in {1..100}; do
  curl -N http://localhost:8082/api/solver/stream &
done
```

## Support & Maintenance

**Repository**: https://github.com/yawningmonsoon/taifoon-solver
**Issues**: https://github.com/yawningmonsoon/taifoon-solver/issues
**Docs**: See README.md and E2E_TESTING.md

---

## Agent Delivery Checklist

- [x] **Agent 1**: Protocol XML Analyzer (31 protocols)
- [x] **Agent 2**: T3RN Sidecar Implementation
- [x] **Agent 3**: Dashboard Builder (Next.js 15)
- [x] **Agent 4**: Executor with Liquidity Waterfall
- [x] **Agent 5**: E2E Integration Testing
- [x] **Agent 6**: Production Deployment Guide ← YOU ARE HERE

**System Status**: ✅ Production Ready

Built with TamTam autonomous delivery 🚀
