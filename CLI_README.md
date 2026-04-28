# Taifoon CLI - Complete Command Reference

**Agent-friendly command-line interface for autonomous cross-chain intent solving**

## Table of Contents

- [Overview](#overview)
- [Installation](#installation)
- [Quick Start](#quick-start)
- [Commands](#commands)
  - [participate](#participate-crown-jewel)
  - [wallet](#wallet)
  - [monitor](#monitor)
  - [execute](#execute)
  - [test](#test)
  - [stats](#stats)
- [Protocol Adapters](#protocol-adapters)
- [Genome SSE Integration](#genome-sse-integration)
- [Agent-Friendly Design](#agent-friendly-design)
- [Environment Variables](#environment-variables)
- [Troubleshooting](#troubleshooting)

---

## Overview

The Taifoon CLI provides a command-line interface for:

- **Autonomous participation** in cross-chain fills with profitability filtering
- **Real-time monitoring** of genome SSE stream for cross-chain intents
- **Wallet management** for solver operations
- **Protocol adapter testing** and connectivity validation
- **Agent-friendly operation** with JSON output mode

Built with Rust for performance and reliability, designed for both human operators and AI agents.

---

## Installation

### Build from source

```bash
cd /Users/mbultra/projects/taifoon-solver
cargo build --release --bin taifoon
```

Binary output: `./target/release/taifoon`

### Verify installation

```bash
./target/release/taifoon --help
```

---

## Quick Start

### 1. Generate a wallet

```bash
# Create new wallet (save the private key!)
./target/release/taifoon wallet new

# Show address from existing key
./target/release/taifoon wallet address --private-key $SOLVER_PRIVATE_KEY
```

### 2. Test connectivity

```bash
# Test all systems
./target/release/taifoon test e2e

# Test individual components
./target/release/taifoon test spinner
./target/release/taifoon test genome
./target/release/taifoon test adapters
```

### 3. Monitor genome stream

```bash
# Watch first 10 intents
./target/release/taifoon monitor --limit 10

# Filter by protocol
./target/release/taifoon monitor --protocol lifi --limit 5

# JSON output for agent consumption
./target/release/taifoon monitor --json --limit 3
```

### 4. Start autonomous participation (Crown Jewel)

```bash
# Dry-run mode (safe testing)
./target/release/taifoon participate \
  --private-key $SOLVER_PRIVATE_KEY \
  --dry-run \
  --min-profit 0.50 \
  --protocol all

# Live autonomous mode
./target/release/taifoon participate \
  --private-key $SOLVER_PRIVATE_KEY \
  --auto \
  --min-profit 1.00 \
  --protocol lifi \
  --max-concurrent 3

# Interactive mode (manual confirmation)
./target/release/taifoon participate \
  --private-key $SOLVER_PRIVATE_KEY \
  --min-profit 0.25 \
  --protocol across
```

---

## Commands

### `participate` (Crown Jewel)

**Authorize with private key and start actively participating in cross-chain fills**

```bash
taifoon participate [OPTIONS]
```

#### Options

| Flag | Env Var | Default | Description |
|------|---------|---------|-------------|
| `--private-key <KEY>` | `SOLVER_PRIVATE_KEY` | *required* | Wallet private key (0x...) |
| `--auto` | - | false | Autonomous mode (no confirmation prompts) |
| `--min-profit <USD>` | - | 0.10 | Minimum profit threshold in USD |
| `--protocol <NAME>` | - | all | Protocol filter (all, lifi, across, mayan, debridge) |
| `--dry-run` | - | false | Simulate fills without broadcasting transactions |
| `--max-concurrent <N>` | - | 3 | Maximum concurrent fills |
| `--json` | - | false | JSON output mode |

#### Examples

**Dry-run testing:**
```bash
./target/release/taifoon participate \
  --private-key 0xabc123... \
  --dry-run \
  --min-profit 0.10 \
  --protocol all
```

**Live autonomous solver:**
```bash
./target/release/taifoon participate \
  --private-key $SOLVER_PRIVATE_KEY \
  --auto \
  --min-profit 1.00 \
  --protocol lifi \
  --max-concurrent 5
```

**Interactive mode (confirm each fill):**
```bash
./target/release/taifoon participate \
  --private-key $SOLVER_PRIVATE_KEY \
  --min-profit 0.50 \
  --protocol across
```

**Agent-friendly JSON output:**
```bash
./target/release/taifoon participate \
  --private-key $SOLVER_PRIVATE_KEY \
  --auto \
  --dry-run \
  --json
```

Output:
```json
{"success":true,"message":"Starting autonomous solver","address":"0x...","auto":true,"dry_run":true}
{"action":"simulated_fill","intent_id":"abc123","protocol":"lifi","estimated_profit":1.23}
```

#### How It Works

1. **Connect to genome SSE stream** (`http://46.4.96.124:30081/api/genome/subscribe/sse`)
2. **Receive intents in real-time** via GenomeClient
3. **Apply protocol filter** (if specified)
4. **Calculate profitability** (gas costs + protocol fees)
5. **Skip if below min-profit threshold**
6. **In interactive mode**: Prompt for confirmation
7. **In dry-run mode**: Simulate execution, log profit estimate
8. **In live mode**: Execute fill via protocol adapter

---

### `wallet`

**Wallet generation and management**

```bash
taifoon wallet <SUBCOMMAND>
```

#### Subcommands

**Generate new wallet:**
```bash
taifoon wallet new [--json]
```

Output:
```
✅ New wallet generated
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
Address:     0x742d35Cc6634C0532925a3b844Bc9e7595f0bEb7
Private Key: 0x1234567890abcdef...

⚠️  SAVE YOUR PRIVATE KEY SECURELY
```

JSON output:
```json
{"success":true,"address":"0x742d35Cc6634C0532925a3b844Bc9e7595f0bEb7","private_key":"0x1234..."}
```

**Show address from existing key:**
```bash
taifoon wallet address --private-key 0xabc123... [--json]
```

Output:
```
Wallet Address: 0x742d35Cc6634C0532925a3b844Bc9e7595f0bEb7
```

JSON output:
```json
{"success":true,"address":"0x742d35Cc6634C0532925a3b844Bc9e7595f0bEb7"}
```

---

### `monitor`

**Monitor genome stream and display intents in real-time**

```bash
taifoon monitor [OPTIONS]
```

#### Options

| Flag | Default | Description |
|------|---------|-------------|
| `--protocol <NAME>` | - | Filter by protocol (lifi, across, mayan, debridge) |
| `--limit <N>` | - | Stop after N intents |
| `--profitable-only` | false | Show only profitable intents |
| `--json` | false | JSON output mode |

#### Examples

**Monitor all intents:**
```bash
./target/release/taifoon monitor
```

**Filter by protocol:**
```bash
./target/release/taifoon monitor --protocol lifi --limit 10
```

**Agent-friendly JSON stream:**
```bash
./target/release/taifoon monitor --json --limit 5
```

Output:
```json
{"success":true,"message":"Monitoring genome stream..."}
{"id":"abc123","protocol":"lifi","src_chain":"ethereum","dst_chain":"optimism","amount":"1000000000000000000","depositor":"0x...","recipient":"0x...","tx_hash":"0x..."}
{"id":"def456","protocol":"across_v3","src_chain":"arbitrum","dst_chain":"base","amount":"5000000000000000000","depositor":"0x...","recipient":"0x...","tx_hash":"0x..."}
```

Human-readable output:
```
📡 Monitoring genome stream: http://46.4.96.124:30081/api/genome/subscribe/sse
   Protocol filter: lifi
   Limit: 10 intents

🎯 Intent #1
   Protocol:   lifi
   Route:      ethereum → optimism
   Amount:     1000000000000000000
   Depositor:  0x1234...
   Recipient:  0x5678...
   Tx Hash:    0xabcd...

🎯 Intent #2
   Protocol:   lifi
   Route:      arbitrum → base
   Amount:     5000000000000000000
   Depositor:  0x9abc...
   Recipient:  0xdef0...
   Tx Hash:    0x1234...
```

---

### `execute`

**Execute a single fill by intent ID**

```bash
taifoon execute <SUBCOMMAND>
```

#### Subcommands

**Execute single fill:**
```bash
taifoon execute fill \
  --intent-id <ID> \
  --private-key <KEY> \
  [--dry-run] \
  [--json]
```

**Example:**
```bash
./target/release/taifoon execute fill \
  --intent-id abc123 \
  --private-key $SOLVER_PRIVATE_KEY \
  --dry-run
```

---

### `test`

**Test connectivity and protocol adapters**

```bash
taifoon test <SUBCOMMAND>
```

#### Subcommands

**Test protocol adapters:**
```bash
taifoon test adapters [--json]
```

Output:
```
🔌 Testing Protocol Adapters
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
Supported protocols: across, across_v3, debridge, dln, lifi, li.fi, mayan, mayan_finance
Total adapters: 8

  ✓ across adapter loaded
  ✓ across_v3 adapter loaded
  ✓ debridge adapter loaded
  ✓ dln adapter loaded
  ✓ lifi adapter loaded
  ✓ li.fi adapter loaded
  ✓ mayan adapter loaded
  ✓ mayan_finance adapter loaded
```

**Test Spinner API connectivity:**
```bash
taifoon test spinner [--json]
```

Output:
```
🌐 Testing Spinner API
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
URL: http://46.4.96.124:30081

  ✓ Spinner API accessible
```

**Test Genome SSE stream:**
```bash
taifoon test genome [--json]
```

Output:
```
📡 Testing Genome SSE Stream
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
URL: http://46.4.96.124:30081/api/genome/subscribe/sse

  ✓ Genome stream accessible
```

**End-to-end test (all systems):**
```bash
taifoon test e2e [--json]
```

Output:
```
🧪 End-to-End Test
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

Stage 1: Adapters...    ✓
Stage 2: Spinner API... ✓
Stage 3: Genome SSE...  ✓
Stage 4: Integration... ✓

All systems ready!
```

---

### `stats`

**Display solver statistics**

```bash
taifoon stats [OPTIONS]
```

#### Options

| Flag | Default | Description |
|------|---------|-------------|
| `--since <DURATION>` | 24h | Time window (1h, 24h, 7d, 30d) |
| `--json` | false | JSON output mode |

#### Examples

```bash
# Last 24 hours
./target/release/taifoon stats

# Last 7 days
./target/release/taifoon stats --since 7d

# JSON output
./target/release/taifoon stats --since 1h --json
```

---

## Protocol Adapters

The CLI supports 8 protocol adapter variants across 4 main protocols:

| Protocol | Variants | Source Code |
|----------|----------|-------------|
| **Across** | `across`, `across_v3` | `crates/protocol-adapters/src/across.rs` |
| **deBridge** | `debridge`, `dln` | `crates/protocol-adapters/src/debridge.rs` |
| **LI.FI** | `lifi`, `li.fi` | `crates/protocol-adapters/src/lifi.rs` |
| **Mayan** | `mayan`, `mayan_finance` | `crates/protocol-adapters/src/mayan.rs` |

### Adapter Trait

All adapters implement the `ProtocolAdapter` trait:

```rust
#[async_trait]
pub trait ProtocolAdapter: Send + Sync {
    fn name(&self) -> &str;
    fn version(&self) -> &str;

    async fn can_fulfill(&self, intent: &Intent) -> Result<bool>;
    async fn estimate_gas(&self, intent: &Intent) -> Result<U256>;
    async fn execute_fill(&self, intent: &Intent, signer: PrivateKeySigner) -> Result<TxHash>;
}
```

### Using Protocol Filters

Protocol filters in `participate` and `monitor` commands are case-insensitive and support partial matching:

```bash
# Match all Across variants (across, across_v3)
--protocol across

# Match all deBridge variants (debridge, dln)
--protocol debridge

# Match all LI.FI variants (lifi, li.fi)
--protocol lifi

# Match all Mayan variants (mayan, mayan_finance)
--protocol mayan

# Match all protocols
--protocol all
```

---

## Genome SSE Integration

### Architecture

```
Genome SSE Stream (http://46.4.96.124:30081/api/genome/subscribe/sse)
         ↓
   GenomeClient (Rust SSE client with auto-reconnect)
         ↓
   tokio::mpsc::channel (async intent queue)
         ↓
   Intent processing loop (filter → profitability → execute)
```

### GenomeClient Features

- **Auto-reconnection**: Retries every 5 seconds on connection loss
- **Stream parsing**: Converts SSE events to Intent structs
- **Error handling**: Graceful degradation on malformed events
- **Async-first**: Built on tokio for non-blocking I/O

### Intent Structure

```rust
pub struct Intent {
    pub id: String,
    pub protocol: String,
    pub src_chain: String,
    pub dst_chain: String,
    pub amount: String,
    pub depositor: String,
    pub recipient: String,
    pub tx_hash: String,
}
```

### SSE Event Format

```json
event: genome_entry
data: {
  "id": "abc123",
  "entity": "proto",
  "chain_id": 1,
  "block_number": 12345678,
  "payload": {
    "Proto": {
      "protocol": "lifi",
      "src_chain": "ethereum",
      "dst_chain": "optimism",
      "amount": "1000000000000000000",
      "depositor": "0x1234...",
      "recipient": "0x5678...",
      "tx_hash": "0xabcd..."
    }
  }
}
```

### Filtering SSE Stream

The genome SSE endpoint supports query parameters:

```bash
# Filter by entity type
curl -N "http://46.4.96.124:30081/api/genome/subscribe/sse?entities=proto,order"

# Filter by chain ID
curl -N "http://46.4.96.124:30081/api/genome/subscribe/sse?chains=1,10,8453"

# Filter by protocol
curl -N "http://46.4.96.124:30081/api/genome/subscribe/sse?protocols=lifi,across_v3"

# Combine filters
curl -N "http://46.4.96.124:30081/api/genome/subscribe/sse?entities=proto&chains=1,10&protocols=lifi"
```

---

## Agent-Friendly Design

### JSON Output Mode

Every command supports `--json` flag for machine-readable output:

```bash
./target/release/taifoon monitor --json --limit 2
```

Output:
```json
{"success":true,"message":"Monitoring genome stream..."}
{"id":"abc123","protocol":"lifi","src_chain":"ethereum","dst_chain":"optimism","amount":"1000000000000000000","depositor":"0x...","recipient":"0x...","tx_hash":"0x..."}
{"id":"def456","protocol":"across_v3","src_chain":"arbitrum","dst_chain":"base","amount":"5000000000000000000","depositor":"0x...","recipient":"0x...","tx_hash":"0x..."}
```

### Exit Codes

- **0**: Success
- **1**: Error (connection failed, invalid key, etc.)
- **2**: No opportunities found (profitable_only mode with no matches)

### Environment Variables

All flags support environment variable overrides:

| Env Var | Flag | Default |
|---------|------|---------|
| `SOLVER_PRIVATE_KEY` | `--private-key` | - |
| `SPINNER_API_URL` | `--spinner-url` | `http://46.4.96.124:30081` |
| `GENOME_SSE_URL` | `--genome-url` | `http://46.4.96.124:30081/api/genome/subscribe/sse` |

**Example:**
```bash
export SOLVER_PRIVATE_KEY=0xabc123...
export SPINNER_API_URL=http://localhost:8081
export GENOME_SSE_URL=http://localhost:8081/api/genome/subscribe/sse

# No need to specify flags
./target/release/taifoon participate --auto --min-profit 1.00
```

### Automated Operation Examples

**Continuous monitoring with systemd:**
```ini
[Unit]
Description=Taifoon Solver Monitor
After=network.target

[Service]
Type=simple
User=solver
Environment=SOLVER_PRIVATE_KEY=0xabc123...
ExecStart=/usr/local/bin/taifoon participate --auto --min-profit 1.00 --protocol all
Restart=always
RestartSec=10

[Install]
WantedBy=multi-user.target
```

**Cron job for periodic stats:**
```bash
# Every hour, log stats
0 * * * * /usr/local/bin/taifoon stats --since 1h --json >> /var/log/taifoon-stats.log
```

**Docker compose:**
```yaml
services:
  taifoon-solver:
    image: taifoon-solver:latest
    command: participate --auto --min-profit 1.00 --protocol lifi --json
    environment:
      - SOLVER_PRIVATE_KEY=${SOLVER_PRIVATE_KEY}
      - SPINNER_API_URL=http://spinner:8081
      - GENOME_SSE_URL=http://spinner:8081/api/genome/subscribe/sse
    restart: unless-stopped
```

---

## Environment Variables

### Global Configuration

```bash
# Required for participation
export SOLVER_PRIVATE_KEY=0x1234567890abcdef...

# Optional overrides
export SPINNER_API_URL=http://46.4.96.124:30081
export GENOME_SSE_URL=http://46.4.96.124:30081/api/genome/subscribe/sse
```

### Per-Command Usage

```bash
# Use env vars
export SOLVER_PRIVATE_KEY=0xabc123...
./target/release/taifoon participate --auto

# Or use flags
./target/release/taifoon participate \
  --private-key 0xabc123... \
  --spinner-url http://localhost:8081 \
  --genome-url http://localhost:8081/api/genome/subscribe/sse \
  --auto
```

---

## Troubleshooting

### Genome SSE Connection Errors

**Symptom:**
```
Failed to connect to genome stream, reconnecting in 5s...
```

**Diagnosis:**

1. **Check if spinner-0 pod is running latest code:**
   ```bash
   ssh root@46.4.96.124
   kubectl get pods -n spinner -l app=spinner
   kubectl logs -n spinner spinner-0 | grep "Protocol genome broadcaster started"
   ```

2. **If broadcaster is missing**, rebuild and deploy:
   ```bash
   # On server
   cd /root/spinner && git pull origin master
   cd /root/spinner/rust && PATH=/root/.cargo/bin:$PATH cargo build --release --bin spinner
   cp /root/spinner/rust/target/release/spinner /tmp/spinner-binary
   cd /tmp && docker build --no-cache -t spinner-monolith:latest -f Dockerfile.spinner-quick .
   docker save spinner-monolith:latest | k3s ctr images import -
   kubectl delete pod -n spinner spinner-0
   ```

3. **Verify genome endpoint is accessible:**
   ```bash
   curl -N http://46.4.96.124:30081/api/genome/subscribe/sse
   ```

   Expected output (SSE stream):
   ```
   event: genome_entry
   data: {"id":"...","entity":"proto",...}
   ```

### Invalid Private Key

**Symptom:**
```
Error: Invalid private key: ...
```

**Solution:**
- Ensure private key starts with `0x`
- Key must be 64 hex characters (32 bytes)
- Use `wallet new` to generate a valid key

### Protocol Filter Not Matching

**Symptom:**
No intents displayed despite genome stream activity.

**Solution:**
- Protocol filters are case-insensitive and partial-match
- Use `--protocol all` to see all intents
- Check protocol names in genome stream: `monitor --json` to see raw protocol values

### Profitability Always Zero

**Symptom:**
All intents show `estimated_profit: 0.0`.

**Current Status:**
Profitability calculation is a placeholder (TODO in code). All intents are currently skipped unless `--dry-run` is used.

**Future Implementation:**
Will integrate with `profit-calc` crate to:
- Fetch real-time gas prices
- Calculate protocol fees
- Estimate spread from DEX prices
- Return actual profit estimates

---

## Source Code Reference

| Component | Path |
|-----------|------|
| CLI entry point | `crates/taifoon-cli/src/main.rs` |
| Participate command | `crates/taifoon-cli/src/execute.rs` |
| Monitor command | `crates/taifoon-cli/src/monitor.rs` |
| Wallet management | `crates/taifoon-cli/src/wallet.rs` |
| Test commands | `crates/taifoon-cli/src/test_mode.rs` |
| GenomeClient | `crates/genome-client/src/lib.rs` |
| Protocol adapters | `crates/protocol-adapters/src/` |
| Genome SSE endpoint | `../spinner/rust/crates/da-api/src/genome_api.rs` |

---

## Contributing

To add new protocol adapters:

1. Implement `ProtocolAdapter` trait in `crates/protocol-adapters/src/`
2. Register in `AdapterFactory::new()` in `crates/protocol-adapters/src/lib.rs`
3. Add protocol name to filter examples in this README
4. Update supported protocol count

---

## License

MIT License - See LICENSE file

---

**Generated with Claude Code** - Part of the Taifoon Solver autonomous cross-chain intent solving system.
