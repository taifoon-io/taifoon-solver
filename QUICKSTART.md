# Taifoon Solver - Quick Start Guide

## 🚀 One-Command Deploy

### Step 1: Push to GitHub

```bash
cd ~/projects/taifoon-solver

# Create private repo and push
gh repo create yawningmonsoon/taifoon-solver --private --source=. --remote=origin --push

# OR manually:
# 1. Create repo at https://github.com/new (yawningmonsoon/taifoon-solver, private)
# 2. Then:
git remote add origin git@github.com:yawningmonsoon/taifoon-solver.git
git branch -M main
git push -u origin main
```

### Step 2: Test Locally

```bash
# Build (takes ~16s)
cargo build --release

# Run (connects to live genome stream)
./target/release/taifoon-solver
```

**Expected Output**:
```
🚀 Taifoon Solver Starting...
📡 Genome SSE: https://api.taifoon.dev/api/genome/subscribe/sse
💰 Min Profit: $1
✅ Loaded 25 protocol fees from solver intel
🔌 Connecting to genome stream...
✅ Connected to genome stream
⏳ Waiting for intents...

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
📥 Intent: lifi_v2:0xabc... (lifi_v2)
   1 → 42161
   Amount: 10000000000 0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48
   User: 0xuser... → 0xuser...
💵 Profit: $43.90
   Protocol Fee: $49.00
   Spread: $0.00
   Gas Cost: $5.10
✅ PROFITABLE - Would execute (executor not yet implemented)
```

## 📊 What It Does Right Now

✅ **Detects Intents**: Consumes genome stream from DA API
✅ **Calculates Profit**: Uses real protocol fees from solver_intel.json
✅ **Filters Opportunities**: Only shows intents > $1 profit
✅ **Logs Everything**: Detailed breakdown of every calculation

⏳ **Missing**: Executor (Phase 3) - doesn't execute fills yet

## 🎯 Current Capabilities

### Supported Protocols (25+)
- **LiFi V2**: 49 bps fees (highest priority)
- **Stargate V2**: 2 bps fees
- **Across V3**: 3 bps fees
- **T3RN LWC**: 5 bps fees
- +21 more protocols

### Profit Calculation
```
Formula: Net Profit = Protocol Fee + Spread - Gas - Liquidity Cost

Example (10K USDC, ETH → Arb):
- Protocol Fee: 10,000 * 49 bps = $49.00
- Gas (ETH): $5.00
- Gas (Arb): $0.10
- Net: $49.00 - $5.10 = $43.90 ✅
```

### Chain Support
- Ethereum (chain 1): $5 gas
- Optimism (chain 10): $0.05 gas
- Base (chain 8453): $0.05 gas
- Arbitrum (chain 42161): $0.10 gas
- +20 more chains

## 🏗️ Project Structure

```
taifoon-solver/
├── crates/
│   ├── genome-client/     ✅ SSE stream consumer
│   ├── profit-calc/       ✅ Profitability filter
│   ├── executor/          ⏳ TODO: Fill execution
│   └── solver-main/       ✅ Main binary
├── config/
│   └── solver_intel.json  ✅ 25+ protocol fees
├── target/release/
│   └── taifoon-solver     ✅ Binary (5MB)
├── README.md              ✅ Full documentation
├── DEPLOY.md              ✅ Deployment guide
├── SESSION_SUMMARY.md     ✅ Build summary
└── QUICKSTART.md          ✅ This file
```

## 🔧 Configuration

### Environment Variables (Optional)
```bash
export GENOME_SSE_URL="https://api.taifoon.dev/api/genome/subscribe/sse"
export MIN_PROFIT_USD="1.0"
# export WALLET_PRIVATE_KEY="..." # Phase 3 only
```

### Update Protocol Fees
Edit `config/solver_intel.json` to adjust protocol fees.

## 📈 Next Steps

### Phase 3: Implement Executor

To actually execute fills, implement `crates/executor/src/lib.rs`:

1. **Hot Wallet Setup**
   ```rust
   let wallet = LocalWallet::from_bytes(&private_key)?;
   let provider = Provider::new(rpc_url);
   ```

2. **LiFi Fill Logic**
   ```rust
   pub async fn fill_lifi_intent(intent: &Intent) -> Result<TxHash> {
       // 1. Check balance on destination
       // 2. Build fill transaction
       // 3. Simulate
       // 4. Execute
       // 5. Wait for confirmation
       // 6. Claim reward
   }
   ```

3. **Profit Tracking**
   ```rust
   let actual_gas = receipt.gas_used;
   let actual_profit = calculate_actual_profit(intent, actual_gas);
   tracing::info!("Actual profit: ${}", actual_profit);
   ```

**Estimated Time**: 2-3 days

## 🧪 Testing

### Unit Tests
```bash
cargo test
# test result: ok. 1 passed; 0 failed
```

### Integration Test (Live Stream)
```bash
# Let it run for 5 minutes to see real intents
timeout 300 ./target/release/taifoon-solver
```

### Profit Calculation Test
```bash
cargo test test_profit_calculation -- --nocapture
# Should show: Net profit: $43.90 for 10K USDC intent
```

## 📋 Troubleshooting

### "Failed to load solver intel"
```bash
# Make sure config file exists
ls config/solver_intel.json

# Should exist. If not:
cp ~/projects/spinner/solver_intel.json config/
```

### "Connection refused" to genome stream
```bash
# Test endpoint directly
curl -N https://api.taifoon.dev/api/genome/subscribe/sse

# Should stream genome events
```

### Build errors
```bash
# Clean and rebuild
cargo clean
cargo build --release
```

## 🎉 Success Criteria

You'll know it's working when you see:

✅ "✅ Loaded N protocol fees from solver intel" (N >= 20)
✅ "✅ Connected to genome stream"
✅ "📥 Intent: ..." (detecting real intents)
✅ "💵 Profit: $..." (calculating profitability)
✅ "✅ PROFITABLE" or "⏭️ SKIP" (filtering)

## 🚦 Status: Phase 1 & 2 COMPLETE

- ✅ **Phase 1**: Genome stream consumer (DONE)
- ✅ **Phase 2**: Profit calculator (DONE)
- ⏳ **Phase 3**: Executor (TODO - 2-3 days)
- ⏳ **Phase 4**: Advanced features (FUTURE)

**Current State**: Production-ready for intent detection and profit analysis. Ready to add execution engine.

---

**Questions?** See README.md for full documentation or SESSION_SUMMARY.md for implementation details.
