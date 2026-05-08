# Testnet Onboarding — Base Sepolia + Solana Devnet

This guide walks a community participant from a clean checkout to a verified testnet fill on Base Sepolia. Use this **before** following `SECURITY_ONBOARDING.md` for the mainnet runbook. Testnet fills do not count for hackathon scoring, but they are the correct place to verify wiring, key handling, and sidecar behavior with no real funds at risk.

> **Companion doc:** mainnet runbook is `SECURITY_ONBOARDING.md`. Once you have completed §6 (Graduation Checklist) of this guide, switch to that document.

---

## 1. Prerequisites

Same toolchain as mainnet, minus the requirement to hold real funds.

| Tool | Version | Install |
|---|---|---|
| Rust toolchain | stable (≥ 1.78) | `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \| sh` |
| `cast` (Foundry) | latest | `curl -L https://foundry.paradigm.xyz \| bash && foundryup` |
| `gh` CLI | ≥ 2.40 | `brew install gh` (macOS) / [cli.github.com](https://cli.github.com) |
| `solana` CLI | ≥ 1.18 | `sh -c "$(curl -sSfL https://release.solana.com/stable/install)"` |
| `sqlite3` | system | preinstalled on macOS / `apt install sqlite3` |
| `jq` | system | `brew install jq` |

After install, verify:

```bash
rustc --version          # rustc 1.78.x or later
cast --version           # cast 0.x.x
gh --version             # gh version 2.x.x
solana --version         # solana-cli 1.18.x or later
```

Build the solver in release mode (same binary handles testnet and mainnet — runtime config decides):

```bash
cd /path/to/taifoon-solver
cargo build --release -p solver-main
```

You do **not** need real ETH or USDC for this guide. You will, however, generate a dedicated testnet signing key. Do not reuse a mainnet key on testnet — testnet RPCs are not always confidential and may log addresses.

---

## 2. Generate a Testnet Wallet & Fund From Faucets

### 2.1 Generate a fresh EVM key

```bash
cast wallet new
# → Successfully created new keypair.
# → Address:     0xabcd...
# → Private key: 0x123...
```

Store the private key in macOS keychain under the same service name as the mainnet doc (`run-testnet.sh` reads from `mamba-messiah-key`):

```bash
security add-generic-password -a "$USER" -s mamba-messiah-key -w 0xYOUR_TESTNET_KEY
```

If you already have a mainnet key in `mamba-messiah-key` and want to keep both, use a separate service for testnet (e.g. `mamba-messiah-key-testnet`) and `export SOLVER_PRIVATE_KEY=...` before running `run-testnet.sh`.

### 2.2 Fund Base Sepolia ETH (gas)

Base provides a public faucet that drops 0.05 ETH per day:

- **https://faucet.base.org** — connect wallet or paste address; requires Coinbase Wallet sign-in for the higher tier.
- Alternative (no Coinbase account): **https://www.alchemy.com/faucets/base-sepolia** (Alchemy account required, drops 0.1 ETH).

Verify receipt:

```bash
cast balance 0xYOUR_ADDR --rpc-url https://sepolia.base.org
# → 50000000000000000   (= 0.05 ETH)
```

You need only ~0.005 ETH for the gas of one fill; the faucet drop is generous.

### 2.3 Fund Base Sepolia USDC (fill capital)

Base Sepolia USDC contract address: `0x036CbD53842c5426634e7929541eC2318f3dCF7e` (Circle official testnet USDC, 6 decimals).

Two faucet options:

- **Circle faucet:** https://faucet.circle.com — select Base Sepolia, paste address; mints 10 USDC per request, no auth.
- **Aave testnet faucet:** https://staging.aave.com/faucet/ — select "Base Sepolia" market, click "Faucet" next to USDC; uses the same Circle USDC contract above.

Verify (USDC has 6 decimals):

```bash
cast call 0x036CbD53842c5426634e7929541eC2318f3dCF7e \
  "balanceOf(address)(uint256)" 0xYOUR_ADDR \
  --rpc-url https://sepolia.base.org
# → 10000000          (= 10 USDC)
```

### 2.4 Fund Arbitrum Sepolia (optional, for cross-chain testing)

`config/chain_wiring.json` wires both Base Sepolia (84532) and Arbitrum Sepolia (421614). For a pure Base→Base self-test you can skip this; for an Arb→Base fill you need a small amount of ETH on Arb Sepolia for relayer reads:

- https://www.alchemy.com/faucets/arbitrum-sepolia — 0.1 ETH per day.

### 2.5 Fund Solana Devnet (skip unless testing Solana later)

Solana Devnet is **not exercised by `run-testnet.sh`** — `PROTOCOL_FILTER="across"` is the default and Across has no Solana presence. If you want to extend testing to Solana later, generate a Solana key and use the built-in faucet:

```bash
solana-keygen new --outfile ~/.config/solana/testnet.json
solana config set --url https://api.devnet.solana.com
solana airdrop 2 --keypair ~/.config/solana/testnet.json
solana balance --keypair ~/.config/solana/testnet.json
# → 2 SOL
```

Devnet airdrop tops out at 2 SOL per request; rate-limited to a few per hour per IP. Fallback web faucets if the CLI rate-limits you: **https://faucet.solana.com** (official) or **https://solfaucet.com**.

---

## 3. Running `run-testnet.sh`

`run-testnet.sh` is the testnet-equivalent of `run-mainnet.sh`. Key differences are all hard-coded in the script (you do not need to override env vars):

| Variable | Testnet value | Mainnet value |
|---|---|---|
| `GENOME_SSE_URL` | `http://127.0.0.1:30081/api/genome/subscribe/sse` (local Spinner) | `https://api.taifoon.dev/api/genome/subscribe/sse` |
| `SPINNER_API_URL` | `http://127.0.0.1:30081` | `https://api.taifoon.dev` |
| `PROTOCOL_FILTER` | `across` (Across only — see §5) | `across,lifi` |
| `MIN_PROFIT_USD` | `0.0` (accept any profitable fill) | `0.50` |
| `OUTCOME_DB_PATH` | `/tmp/taifoon_testnet_outcomes.sqlite` | `./outcomes/mainnet_<ts>.sqlite` |
| `WALLET_DB_PATH` | `/tmp/taifoon_testnet_wallet.sqlite` | `./outcomes/wallet_mainnet.sqlite` |
| `RUST_LOG` | `taifoon_solver=debug,executor=debug,…` | `…=info,…` |

### 3.1 Local Spinner requirement

Testnet mode points at a **local** Spinner stream at `127.0.0.1:30081`. If you don't have Spinner running locally, the solver will start up cleanly but produce no intents. Either:

- Run Spinner locally (see the Spinner repo at `/Users/mbultra/projects/spinner` for its testnet runbook), **or**
- Override with the public stream: `GENOME_SSE_URL=https://api.taifoon.dev/api/genome/subscribe/sse SPINNER_API_URL=https://api.taifoon.dev ./run-testnet.sh` — note that public-stream intents target **mainnet** chain IDs, so you will see `chain_filter` skips for everything.

### 3.2 Dry-run first

Always dry-run before live broadcast on testnet:

```bash
DRY_RUN=true ./run-testnet.sh
# or:
./run-testnet.sh --dry-run
```

### 3.3 Expected startup banner

A healthy testnet startup prints (lines from `crates/solver-main/src/main.rs:122–243`, abbreviated):

```
🚀 Taifoon Solver Starting...
📡 Genome SSE: http://127.0.0.1:30081/api/genome/subscribe/sse
🔌 Spinner API: http://127.0.0.1:30081
🎯 Protocol filter: across
💰 Min Profit: $0
🧪 DRY_RUN: false        ← if you set DRY_RUN=true this reads "true"
💾 Outcome DB: /tmp/taifoon_testnet_outcomes.sqlite
🌐 API Port: 8082
✅ Solver event API on :8082
📐 skip-rules: MAMBA_LAKE_URL unset, no rules loaded
📐 skip-rules active: 0
💼 Wallet manager: db=/tmp/taifoon_testnet_wallet.sqlite budget=$10000
✅ Lambda controller live — solver=0xYOUR_TESTNET_ADDR
♻️  Rebalancer background task started (interval=300s dry_run=…)
🔁 deBridge claim-retry background task started (interval=300s)
✅ Genome SSE + deBridge on-chain + Across + Mayan pollers started
⏳ Waiting for intents...
```

### 3.4 How to confirm testnet (not mainnet)

Three independent checks:

1. **Spinner URL** — banner line must read `http://127.0.0.1:30081/...` (or your override). Mainnet reads `https://api.taifoon.dev/...`.
2. **Outcome DB path** — `💾 Outcome DB: /tmp/taifoon_testnet_outcomes.sqlite`. Mainnet writes under `./outcomes/mainnet_YYYYMMDD_HHMMSS.sqlite`.
3. **Wallet pre-flight** — at startup, `run-testnet.sh` prints `Base Sepolia balance: <wei>` after deriving your address. Mainnet's pre-flight queries Base mainnet (`https://mainnet.base.org`). If you see `Base Sepolia balance: ...` you are on testnet.

If any of those three look wrong, **kill the solver immediately** (`pkill -SIGTERM taifoon-solver`) before any broadcast can happen.

---

## 4. Verifying a Testnet Fill

When the solver successfully fills an Across intent, the on-chain side deposits to the Across SpokePool on Base Sepolia (`0x82B564983aE7274c86695917BBf8C99ECb6F0F8F`, from `config/chain_wiring.json`).

### 4.1 What a successful fill looks like in logs

The successful-fill log line (from `main.rs:780`) is:

```
🎉 across_v3 fill confirmed: 0x<64-hex-tx-hash>
```

The line immediately preceding it shows the intent that triggered the fill:

```
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
📥 <intent_id> (across_v3) <src_chain>→<dst_chain> amt=<input_amount>
```

### 4.2 Verify on Base Sepolia Basescan

Base Sepolia block explorer URL pattern:

```
https://sepolia.basescan.org/tx/<TX_HASH>
```

Open the URL — you should see:
- **Status:** Success
- **To:** `0x82B564983aE7274c86695917BBf8C99ECb6F0F8F` (Across V3 SpokePool, Base Sepolia)
- **Method:** `fillV3Relay` (or `fillRelay` for the V3.5 selector `0xdeff4b24`)
- **From:** your testnet solver address from §2.1

For the underlying USDC transfer, click the **Tokens Transferred** section — there should be a `USDC` row showing your address sending the fill amount to the depositor.

### 4.3 Verify via the outcome SQLite

```bash
sqlite3 /tmp/taifoon_testnet_outcomes.sqlite \
  "SELECT created_at, intent_id, protocol, fill_tx_hash, profit_usd
   FROM solver_outcomes
   WHERE decision = 'broadcast'
   ORDER BY created_at DESC LIMIT 5;"
```

Each broadcast row has `fill_tx_hash` populated; cross-check it matches the Basescan tx.

### 4.4 Verify via the dashboard endpoint

The solver-event API exposes recent outcomes at:

```bash
curl -s http://127.0.0.1:8082/api/solver/outcomes | jq '.outcomes[] | select(.decision == "broadcast")'
```

Each entry has `fill_tx_hash` and a `decision` field — useful if you want to confirm fills without leaving the terminal.

---

## 5. Testnet vs Mainnet Differences

These are the deliberate divergences in `run-testnet.sh` and `config/chain_wiring.json`:

| Behavior | Testnet | Mainnet |
|---|---|---|
| Protocol filter | `across` only | `across,lifi` (configurable) |
| Mayan / Solana fills | Not exercised — Mayan has no testnet deployment | Mayan Swift active on Solana mainnet |
| deBridge fills | Not exercised — deBridge DLN is mainnet-only on the chains we wire | `claimUnlock` retry sidecar active |
| LiFi fills | Not exercised — testnet stream is too sparse for the LiFi resolver | li.quest API path active |
| Genome stream | Local Spinner (`127.0.0.1:30081`) by default | `api.taifoon.dev` |
| `MIN_PROFIT_USD` | `0.0` (accept any positive profit) | `0.50` |
| `MAX_NOTIONAL_USD` | Not enforced (script does not export it; falls back to controller default) | `200` (script-enforced) |
| Outcome DB | `/tmp/taifoon_testnet_outcomes.sqlite` (volatile — `/tmp` is wiped on reboot) | `./outcomes/mainnet_<ts>.sqlite` (persistent) |
| Wallet pre-flight | Base Sepolia only | Base + Arb + Optimism + Solana |
| `RUST_LOG` | `debug` (verbose, useful for first-run learning) | `info` |
| Across SpokePool | `0x82B564983aE7274c86695917BBf8C99ECb6F0F8F` (Base Sepolia) | `0x09aea4b2242abC8bb4BB78D537A67a245A7bEC64` (Base mainnet) |
| USDC contract | `0x036CbD53842c5426634e7929541eC2318f3dCF7e` (Circle Sepolia) | `0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913` (Base mainnet) |

**Operational implications:**

- **Outcome DB lives in `/tmp`** — copy it to a persistent location if you want to keep testnet history across reboots: `cp /tmp/taifoon_testnet_outcomes.sqlite ./outcomes/testnet_$(date +%Y%m%d).sqlite`.
- **No Mayan / no deBridge / no LiFi on testnet** means you cannot exercise the §7.3 (deBridge claim verification) or §7.1 (Mayan auction) flows from `SECURITY_ONBOARDING.md` until you switch to mainnet. If you need to test the deBridge claim sidecar, run `DRY_RUN=true ./run-mainnet.sh` and read the dry-run log — no broadcasts happen but the sidecar's claim-retry tick will print what it *would* have done.
- **The default testnet flow only exercises Across.** That is intentional: Across has the cleanest testnet deployment, the broadest chain coverage, and the simplest fee model (atomic, no claim).

---

## 6. Graduation Checklist — Testnet → Mainnet

Move from `run-testnet.sh` to `run-mainnet.sh` only after all of the following pass. Each item maps to a check in `SECURITY_ONBOARDING.md` §11.

- [ ] **A testnet fill is recorded.** `sqlite3 /tmp/taifoon_testnet_outcomes.sqlite "SELECT count(*) FROM solver_outcomes WHERE decision='broadcast';"` returns ≥ 1.
- [ ] **The fill is verifiable on Basescan.** You can open `https://sepolia.basescan.org/tx/<HASH>` and see Status: Success.
- [ ] **The dashboard endpoint returns the outcome.** `curl -s http://127.0.0.1:8082/api/solver/outcomes | jq '.outcomes | length'` returns ≥ 1.
- [ ] **Dry-run on mainnet has been run and reviewed.** `DRY_RUN=true ./run-mainnet.sh` runs to "Waiting for intents..." and you have read at least 5 minutes of `⏭️ skip` reasons without anything looking unexpected.
- [ ] **Mainnet keychain entry is set.** `security find-generic-password -s mamba-messiah-key -w` prints your mainnet key (or it is set via `SOLVER_PRIVATE_KEY` env at run time). The address it derives to is **not** the testnet address from §2.1.
- [ ] **Solana keychain entry is set if you want Mayan fills.** `security find-generic-password -s mamba-messiah-solana-key -w` prints a Solana key. Skip if you want EVM-only on first mainnet run.
- [ ] **Mainnet wallet is funded with the capital you are willing to lose.** Recommended floor: $20 USDC + $5 worth of ETH per chain you want to fill on. Do not start above $50 total on first mainnet run.
- [ ] **`MAX_NOTIONAL_USD` for first run is small.** Override on the command line: `DRY_RUN=false MAX_NOTIONAL_USD=5 ./run-mainnet.sh`. Do not use the script default of $200 on the very first live run.
- [ ] **`PROTOCOL_FILTER` and `DST_CHAIN_FILTER` are scoped to where you have funds.** First mainnet run should set `PROTOCOL_FILTER=across` and `DST_CHAIN_FILTER=8453` (Base only) to limit blast radius.
- [ ] **You have read SECURITY_ONBOARDING.md §7 (Fee Collection) end-to-end** — you understand which protocols auto-claim and which require the deBridge sidecar.
- [ ] **You have a kill-switch ready.** Test it: open a second terminal, run `pkill -SIGTERM taifoon-solver`, confirm the solver exits within 1–2 seconds. You will need this on mainnet.
- [ ] **`git status` shows no `.env` or key material staged** before you `DRY_RUN=false` for the first time.

When all checkboxes are ticked:

```bash
DRY_RUN=false MAX_NOTIONAL_USD=5 PROTOCOL_FILTER=across DST_CHAIN_FILTER=8453 ./run-mainnet.sh
```

The first **mainnet** fill is the hackathon-graded artifact. After it confirms on Basescan and lands in `./outcomes/mainnet_<ts>.sqlite`, you can broaden `PROTOCOL_FILTER`, raise `MAX_NOTIONAL_USD`, and follow the live-ops cadence in `SECURITY_ONBOARDING.md` §11.
