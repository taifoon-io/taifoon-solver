# Taifoon Solver — Security & Onboarding Guide

**Audience:** Open-source community participants joining through the Solana Colosseum or Frontier hackathon tracks.  
**Scope:** Key management policy, environment setup, operational risk controls, and what you must never do.  
**Last updated:** 2026-05-08

---

## 1. What You Are Operating

Taifoon is a cross-chain intent solver. When you run it in live mode (`DRY_RUN=false`) it:

- Holds real funds in a hot wallet (EVM address + Solana address)
- Signs and broadcasts fill transactions on mainnet (Across, deBridge, LiFi, Mayan)
- Claims protocol fees after fills complete

**If your key is compromised, your fill capital is gone.** There is no recovery path. Treat this like running a DeFi protocol, not a demo app.

---

## 2. Private Key Policy

### 2.1 Two keys required

| Key | Purpose | Format |
|-----|---------|--------|
| `SOLVER_PRIVATE_KEY` | EVM signing (secp256k1) — signs all Across, deBridge, LiFi, Mayan-EVM fills | `0x` + 64 hex chars |
| `SOLANA_PRIVATE_KEY` | Solana signing (ed25519) — signs Mayan Swift Solana-side fills | base58 64-byte keypair (as output by `solana-keygen`) |

Solana key is **optional** — if absent, Mayan fills to Solana destinations are skipped. The solver still runs.

### 2.2 The only approved storage method

**macOS keychain (Keychain Services)**. The solver's built-in `MESSIAH` loader reads keys directly from the keychain via the `security` CLI — the raw key string lives in memory for less than one microsecond before being parsed into a signer object and explicitly dropped. It is never written to disk, logged, or passed through environment variables in the canonical path.

```bash
# Store EVM key
security add-generic-password \
  -a "$USER" \
  -s mamba-messiah-key \
  -w "0xYOUR_64_HEX_PRIVATE_KEY"

# Store Solana key (base58 64-byte keypair from solana-keygen)
security add-generic-password \
  -a "$USER" \
  -s mamba-messiah-solana-key \
  -w "YOUR_BASE58_SOLANA_KEYPAIR"
```

**Linux / CI**: use a secrets manager (HashiCorp Vault, AWS Secrets Manager, GitHub Actions secrets injected at runtime). Set the env vars in the process environment — never write them to files on disk. See section 2.3 for the env var fallback.

### 2.2.1 Linux / CI secrets — concrete patterns

**GitHub Actions** (recommended for CI pipelines):
```yaml
# .github/workflows/solver.yml
- name: Run solver dry-run
  env:
    SOLVER_PRIVATE_KEY: ${{ secrets.SOLVER_PRIVATE_KEY }}
    SOLANA_PRIVATE_KEY: ${{ secrets.SOLANA_PRIVATE_KEY }}
  run: cargo run -p solver-main -- --dry-run --max-events 3
```
Store keys via: **Settings → Secrets and variables → Actions → New repository secret**.
Keys injected this way are masked in logs and never written to disk.

**HashiCorp Vault** (recommended for persistent Linux deployments):
```bash
# Authenticate once (e.g. via AppRole in production)
vault login -method=token token="$VAULT_TOKEN"

# Inject at solver start — keys never touch disk
export SOLVER_PRIVATE_KEY="$(vault kv get -field=evm_key secret/taifoon-solver)"
export SOLANA_PRIVATE_KEY="$(vault kv get -field=solana_key secret/taifoon-solver)"
./run-mainnet.sh
```
Use `VAULT_ADDR`, `VAULT_TOKEN` (or AppRole `VAULT_ROLE_ID`/`VAULT_SECRET_ID`) from your platform's env-injection mechanism.

**AWS Secrets Manager**:
```bash
# Requires aws-cli v2 and IAM permission secretsmanager:GetSecretValue
export SOLVER_PRIVATE_KEY="$(aws secretsmanager get-secret-value \
  --secret-id taifoon-solver/evm-key \
  --query SecretString \
  --output text)"
export SOLANA_PRIVATE_KEY="$(aws secretsmanager get-secret-value \
  --secret-id taifoon-solver/solana-key \
  --query SecretString \
  --output text)"
./run-mainnet.sh
```

In all cases: inject into the process environment immediately before launching the solver and let the subprocess inherit it — never write the value to a file.

### 2.3 Env var fallback (acceptable but lower security)

If keychain entries are not found, the solver falls back to:

```bash
SOLVER_PRIVATE_KEY=0x...
SOLANA_PRIVATE_KEY=...
```

This is acceptable for short-lived CI runs or hackathon demos where the environment is ephemeral. It is **not acceptable** for persistent deployments because:
- The key is visible in `/proc/<pid>/environ` to same-user processes (Linux)
- Shell history may capture it if typed directly
- It can appear in error messages from some shells

If you use env vars, always load them from a secrets manager at runtime — do not write them to `.env` files or shell RC files.

### 2.4 What you must never do

| Action | Why |
|--------|-----|
| `--private-key 0x...` on the CLI | Key visible in `ps aux` to all users on the machine |
| Commit `.env` containing keys | Git history is permanent; public repos are indexed immediately |
| Hardcode keys in any source file | Same as above |
| Share keys in Slack, Discord, GitHub issues | No message is private enough for a live signing key |
| Run the solver on a shared machine | Any co-user can read your process environment |
| Copy keys to clipboard without clearing it | Clipboard managers log history |

### 2.5 Generating keys safely

**EVM:**
```bash
# Foundry (recommended — offline)
cast wallet new

# Or via the taifoon CLI (generates and prints — copy immediately, clear terminal)
cargo run --bin taifoon -- wallet new
```

**Solana:**
```bash
# Official Solana toolchain
solana-keygen new --no-bip39-passphrase --outfile /tmp/solver-solana.json
# Read the base58 secret key — it is the value you store in keychain
cat /tmp/solver-solana.json   # array of bytes → base58 is the 88-char string
# Delete the file after storing in keychain
rm /tmp/solver-solana.json
```

Do **not** use browser-based key generators or online tools.

---

## 3. Environment Variables Reference

### 3.1 Secrets (handle with care)

| Variable | Required | Description |
|----------|----------|-------------|
| `SOLVER_PRIVATE_KEY` | Yes (live mode) | EVM secp256k1 private key. Loaded from keychain by `run-mainnet.sh`; fallback to env var. |
| `SOLANA_PRIVATE_KEY` | No | Solana ed25519 keypair (base58, 64 bytes). Without it, Solana-destination Mayan fills are skipped. |
| `SOLANA_RPC_URL` | No (recommended in production) | Solana JSON-RPC endpoint used by the Mayan-Solana broadcaster. Defaults to the public endpoint `https://api.mainnet-beta.solana.com`, which is rate-limited and unsuitable for sustained fills. Provision a private endpoint and set this variable for production. |

> **Note on `SOLANA_RPC_URL`:** The default in `run-mainnet.sh` is the public Solana RPC (`https://api.mainnet-beta.solana.com`). It will work for dry-runs and low-volume probing, but the public endpoint enforces aggressive rate limits and may drop requests during congestion — a Mayan-Solana fill that misses its priority window because of a 429 response is a lost fill. For production use, provision your own endpoint at [helius.xyz](https://www.helius.dev/) (or any equivalent provider) and export `SOLANA_RPC_URL=https://mainnet.helius-rpc.com/?api-key=YOUR_KEY`. The startup banner prints a `WARN: Using public Solana RPC` line whenever the default is in effect, so you'll always know which endpoint a given run targeted. Treat the API-key URL as a secret — same care as any other RPC URL containing credentials (see §4).

### 3.2 Operational controls

| Variable | Default | Description |
|----------|---------|-------------|
| `DRY_RUN` | `true` | **Must be `false` to broadcast.** In dry-run mode, all fills are simulated — no transactions leave the wallet. Always start here. |
| `SIMULATION_MODE` | mirrors `DRY_RUN` | Additional simulation gate inside the executor. Both must be `false` for live fills. |
| `MAX_NOTIONAL_USD` | `200` | Hard cap per fill (USD). The solver refuses to fill any intent above this amount regardless of profit. Start at $5–$20 while testing. |
| `MIN_PROFIT_USD` | `0.50` | Minimum net profit to attempt a fill. Below this the intent is skipped after gas estimation. |
| `MAX_INPUT_USD` | `15` | Amount cap applied at intake (before any RPC calls). Intents above this never reach the executor. |
| `MIN_INPUT_USD` | `0.50` | Dust floor — intents below this are dropped immediately. |
| `PROTOCOL_FILTER` | `across,lifi` | Comma-separated list of protocols to handle. Options: `across`, `lifi`, `debridge`, `mayan`. |
| `DST_CHAIN_FILTER` | `8453` | Only fill intents whose output chain is in this comma-separated list. `8453`=Base, `42161`=Arbitrum, `10`=Optimism. Empty = all wired chains. |
| `SIDECAR_INTERVAL_SECS` | `300` | How often the rebalancer and deBridge claim-retry run (seconds). |

### 3.3 Infrastructure

| Variable | Default | Description |
|----------|---------|-------------|
| `CHAIN_WIRING_JSON` | — | JSON map of chain ID → RPC URL + contract addresses. Required for live fills. See section 4. |
| `CHAIN_WIRING_FILE` | — | Path to a JSON file with the same schema as `CHAIN_WIRING_JSON`. |
| `GENOME_SSE_URL` | `https://api.taifoon.dev/api/genome/subscribe/sse` | Intent event stream endpoint. |
| `SPINNER_API_URL` | `https://api.taifoon.dev` | Gas estimation and fill-permit API. |
| `SOLANA_ADDRESS` | derived from `SOLANA_PRIVATE_KEY` | Solver's Solana public key. Auto-derived by `run-mainnet.sh`; set explicitly if not running the script. |
| `OUTCOME_DB_PATH` | `./outcomes/mainnet_<timestamp>.sqlite` | SQLite file for per-fill traces. Contains tx hashes and profit, no secrets. |
| `WALLET_DB_PATH` | `./outcomes/wallet_mainnet.sqlite` | SQLite file for intent lifecycle state. No secrets. |
| `RUST_LOG` | `taifoon_solver=info,...` | Tracing filter. Do not set to `trace` in production — it can log RPC request bodies. |
| `SOLVER_API_TOKEN` | auto-generated at boot | Bearer token gating every `/api/solver/*` route (issue #8). See **API authentication** below. |

**API authentication.** Every route under `/api/solver/*` requires the header
`Authorization: Bearer $SOLVER_API_TOKEN`. Requests without the header,
with the wrong scheme, or with a mismatched token receive
`401 Unauthorized` with body `{"error":"unauthorized"}`. The `/health`
endpoint is intentionally exempt so monitoring and load balancers can probe
without credentials.

If `SOLVER_API_TOKEN` is not set in the environment when the solver starts,
a 32-byte hex token is generated from `/dev/urandom`, exported into the
process environment, and printed once to stdout in this format:

```
────────────────────────────────────────────────────────────────────
SOLVER API TOKEN: 8a4f…<60 hex chars>…c1b2
(auto-generated; set SOLVER_API_TOKEN to override before next start)
────────────────────────────────────────────────────────────────────
```

The auto-generated path is the dev/local fallback. **In production set
`SOLVER_API_TOKEN` explicitly before launch** — otherwise a restart rotates
the token and breaks every dashboard / proxy / cron job that has it cached.
For the dashboard proxy at `dashboard/app/api/solver/[...path]/route.ts`,
mirror the same token into `SOLVER_API_TOKEN` on the dashboard's
environment so requests are forwarded with the correct header.

Quick smoke-test:

```bash
# Without token — must return 401
curl -s -o /dev/null -w "%{http_code}\n" http://localhost:8082/api/solver/outcomes
# 401

# With token — must return 200 + JSON array
curl -s -H "Authorization: Bearer $SOLVER_API_TOKEN" http://localhost:8082/api/solver/outcomes | head -c 80

# Health probe — never gated
curl -s -o /dev/null -w "%{http_code}\n" http://localhost:8082/health
# 200
```

### 3.4 Optional API keys

| Variable | Description |
|----------|-------------|
| `LIFI_API_KEY` | LiFi li.quest API key. Without it, the solver uses the public (unauthenticated) rate limit for bridge resolution. Get one at li.fi/developers. |
| `DEBRIDGE_WS_API_KEY` | deBridge WebSocket API key. Without it, the solver uses a shared default key which is rate-limited. |

---

## 4. Chain Wiring

The executor needs to know which RPC endpoint and contract addresses correspond to each chain. Pass this as JSON:

```bash
export CHAIN_WIRING_JSON='{
  "8453": {
    "rpc_url": "https://mainnet.base.org",
    "spoke_pool": "0x09aea4b2242abC8bb4BB78D537A67a245A7bEC64",
    "operator": "0x0000000000000000000000000000000000000000"
  },
  "42161": {
    "rpc_url": "https://arb1.arbitrum.io/rpc",
    "spoke_pool": "0xe35e9842fceaCA96570B734083f4a58e8F7C5f2A",
    "operator": "0x0000000000000000000000000000000000000000"
  },
  "10": {
    "rpc_url": "https://mainnet.optimism.io",
    "spoke_pool": "0x6f26Bf09B1C792e3228e5467807a900A503c0281",
    "operator": "0x0000000000000000000000000000000000000000"
  }
}'
```

**If your RPC URLs contain API keys** (Alchemy, Infura, Helius), treat `CHAIN_WIRING_JSON` with the same care as a secret — do not commit it or log it.

---

## 5. Running the Solver

### 5.1 First run — dry-run only

```bash
# Build
cargo build --release --bin taifoon-solver

# Store keys in keychain (macOS)
security add-generic-password -a "$USER" -s mamba-messiah-key -w "0xYOUR_EVM_KEY"

# Run in dry-run mode (safe — no broadcasts)
./run-mainnet.sh
```

The banner will show:
```
 Mode: DRY-RUN (no broadcasts)
```

Watch the logs. You should see `⏭️ amount_cap skip` or `✅ estimate gate passed` entries. No transactions are sent.

### 5.2 Going live

Only after you have verified dry-run output and funded the solver wallet:

```bash
DRY_RUN=false MAX_NOTIONAL_USD=10 ./run-mainnet.sh
```

The script will print pre-flight balances and ask for confirmation before broadcasting.

**Minimum viable capital per chain:**
- Base: $5 USDC + 0.001 ETH (gas)
- Arbitrum: $5 USDC + 0.0005 ETH (gas)
- Solana: 0.01 SOL (gas for Mayan fills)

### 5.3 Kill switch

```bash
pkill -SIGTERM taifoon-solver
```

The solver handles SIGTERM gracefully — in-flight fills complete, then it exits. SIGKILL (`pkill -9`) is safe but may leave the wallet DB in an inconsistent state (resolved on next start).

---

## 6. Capital Risk Controls

The solver has multiple layered guards that run in sequence before any broadcast:

| Layer | Guard | What it blocks |
|-------|-------|----------------|
| 1 | `MIN_INPUT_USD` / `MAX_INPUT_USD` | Drops intents outside the configured range at intake |
| 2 | `amount_cap` check | Hard cap at `MAX_NOTIONAL_USD` before any enrichment |
| 3 | Dedup cache | Skips intents already dispatched within this process lifetime |
| 4 | Protocol filter | Only processes protocols in `PROTOCOL_FILTER` |
| 5 | Spinner permit | Estimates gas, checks profitability against `MIN_PROFIT_USD` |
| 6 | ERC-20 balance check | Skips if wallet lacks the output token |
| 7 | ETH balance check | Skips native-output fills if ETH < required amount |
| 8 | Exclusivity guard | Respects exclusive-relayer windows |
| 9 | `DRY_RUN` gate | Final gate — if true, logs intent and stops (no broadcast) |

**The most important control for new operators is `MAX_NOTIONAL_USD`.** Set it to $5–$10 while getting familiar with the system. A misconfigured solver can drain its wallet quickly on large intents.

---

## 7. Fee Collection

The solver earns revenue from per-fill protocol fees. Each protocol has a different
fee-flow model — some pay out at fill time, others require a follow-up claim
transaction. Understand which protocols you are filling for so you know where to
look for your earnings and what to do when an automated claim stalls.

### 7.1 Per-protocol fee flow

| Protocol | Fee mechanism | Claim required? | Where the fee lands |
|---|---|---|---|
| **Across** | Relay fee (input − output) is captured by the solver at the moment `fillRelay` lands. The solver receives `outputAmount` of the destination token and is reimbursed by the Across hub for `inputAmount` of the source token; the spread is the relay fee. | **No** — atomic at fill time. | Solver wallet on the source chain (reimbursement leg lands ~30s–2min after fill). |
| **deBridge** | Solver fronts the `takeAmount` on the destination chain, then must call `claimUnlock()` on the **source** chain to release the locked `giveAmount` back to its wallet. The portfolio sidecar runs the claim loop every `SIDECAR_INTERVAL_SECS`. | **Yes** — must call `claimUnlock(orderId, ...)`. Automated by the sidecar; operator should still verify on-chain. | Solver wallet on the source chain after the claim lands. |
| **Mayan** | Auction-based — protocol auto-distributes the auction-winning fee to the registered solver address when the auction resolves on Solana. No solver-initiated claim. | **No** — automatic on auction resolution. | Solver Solana address; verify on Solscan after the auction window closes. |
| **LiFi** | LiFi's relayer captures the spread; the solver earns the fill fee built into the calldata at execution time. | **No** — atomic at fill time. | Solver wallet on the destination chain (same tx as the fill). |

### 7.2 Expected latency between fill and fee receipt

| Protocol | Typical fee-receipt latency |
|---|---|
| Across | **~0s** at fill (output token), **30s–2min** for the source-chain reimbursement leg |
| deBridge | **~1–5 min** — bounded by `SIDECAR_INTERVAL_SECS` (default 300s); first claim attempt happens on the next sidecar tick after the fill |
| Mayan | **~auction duration** — variable, typically 30s–3min depending on auction parameters and Solana load |
| LiFi | **~0s** — same transaction as the fill |

If a fee has not landed within 2× the expected latency, treat it as anomalous and
investigate (especially for deBridge — see 7.3).

### 7.3 Verifying a deBridge claim

deBridge is the only protocol where the solver runs a follow-up transaction, so
it's the only protocol where claim drift is a real failure mode. After a fill,
verify the claim landed using these steps:

**(a) Find the fill in the outcome SQLite.**

```bash
sqlite3 ./outcomes/mainnet_*.sqlite \
  "SELECT intent_id, protocol, fill_chain_id, fill_tx_hash, claim_tx_hash, created_at \
   FROM fills WHERE protocol = 'debridge' ORDER BY created_at DESC LIMIT 10"
```

**(b) Check `claim_tx_hash` is populated.**

If `claim_tx_hash` is `NULL` and the row is older than `2 × SIDECAR_INTERVAL_SECS`
(default: 10 min), the automated claim has not yet succeeded. The sidecar logs
will show retry attempts — grep them:

```bash
grep -E "debridge.*claim" ./logs/solver-*.log | tail -50
```

**(c) Verify on the source-chain explorer.**

Take the `claim_tx_hash` and look it up on the source chain's explorer
(Etherscan for chain 1, Arbiscan for 42161, Basescan for 8453, etc.). Confirm:
- Tx status: **Success**
- Method: `claimUnlock` (or `claim` depending on the deBridge router version)
- The `giveAmount` of the locked token was transferred to your solver wallet
- No `ERC20: insufficient allowance` or `OrderAlreadyClaimed` revert

If the tx reverted, the funds may be recoverable via manual claim (see 7.4) or
may have been claimed by another solver in a race — check the OrderId on the
deBridge explorer (`https://app.debridge.finance/orders/<orderId>`).

### 7.4 Manual deBridge claim recovery

If the auto-claim has failed repeatedly (sidecar logs show retries returning
errors), recover manually:

**(a) Find pending claims:**

```bash
sqlite3 ./outcomes/mainnet_*.sqlite <<'SQL'
SELECT intent_id,
       fill_chain_id        AS dst_chain,
       src_chain,
       fill_tx_hash,
       json_extract(extra, '$.order_id')        AS order_id,
       json_extract(extra, '$.give_chain_id')   AS give_chain,
       json_extract(extra, '$.dln_source')      AS dln_source_addr,
       created_at
FROM fills
WHERE protocol = 'debridge'
  AND claim_tx_hash IS NULL
  AND created_at < datetime('now', '-10 minutes')
ORDER BY created_at ASC;
SQL
```

**(b) Issue the claim manually with `cast`.**

You need:
- `RPC_URL` for the source chain (the chain where the deBridge `DlnSource`
  contract holds the locked `giveAmount`)
- `DLN_SOURCE` — the `DlnSource` contract address on that chain
- `ORDER_ID` — 32-byte order id from the SQL above
- `SOLVER_PRIVATE_KEY` (loaded from keychain)

```bash
# Example: claim on Arbitrum (chain 42161)
RPC_URL="https://arb1.arbitrum.io/rpc"
DLN_SOURCE="0xeF4fB24aD0916217251F553c0596F8Edc630EB66"
ORDER_ID="0xabc123..."   # from the SQL above

# Pull the EVM key from keychain
SOLVER_PK=$(security find-generic-password -s mamba-messiah-key -w)

# Submit claimUnlock — the second arg (cancelBeneficiary) is the solver address
SOLVER_ADDR=$(cast wallet address --private-key "$SOLVER_PK")

cast send \
  --rpc-url "$RPC_URL" \
  --private-key "$SOLVER_PK" \
  "$DLN_SOURCE" \
  "claimUnlock(bytes32,address)" \
  "$ORDER_ID" \
  "$SOLVER_ADDR"

# Clear the key from the shell — do this immediately
unset SOLVER_PK
```

**(c) Update the outcome row** so the sidecar stops retrying:

```bash
sqlite3 ./outcomes/mainnet_*.sqlite \
  "UPDATE fills SET claim_tx_hash = '0xYOUR_CLAIM_TX' WHERE intent_id = 'YOUR_INTENT_ID'"
```

If `claimUnlock` itself reverts with `OrderNotFulfilled` or
`ClaimAuthorityMismatch`, the order was filled by a different solver or the
fulfill leg never landed — open an issue with the `intent_id`, both tx hashes,
and the revert reason; do not retry blindly.

---

## 8. What Is and Is Not Logged

### Never logged
- Raw private key strings
- Solana signing key bytes
- Authorization header values
- RPC URL contents (but error messages from failed RPC calls may include the URL)

### Always logged (public information)
- Solver EVM address (derived public key)
- Solver Solana address (public key)
- Transaction hashes (public on-chain)
- Intent IDs, amounts, chain IDs
- Skip reasons and profit calculations

### Log level warning
Do not set `RUST_LOG=trace` in production. The `trace` level in some dependencies logs full HTTP request/response bodies, which can include RPC responses containing your address and balance details (not key material, but operational intelligence).

---

## 9. Wallet Hygiene for Community Participants

> **New to the solver?** Walk through [TESTNET_ONBOARDING.md](TESTNET_ONBOARDING.md) first — it covers prerequisites, faucet links for Base Sepolia and Solana Devnet, how to confirm you are connected to testnet (not mainnet), and a graduation checklist that hands off to this document. Going straight to mainnet without verifying the wiring on testnet is the single most common avoidable mistake.

### Use a dedicated solver wallet

Do not reuse a personal wallet or any wallet holding significant assets as the solver signing key. The solver is a hot wallet by design. It must:
- Hold only the capital allocated for filling (e.g., $20–$200 USDC)
- Have a small ETH/gas buffer per chain
- Never hold NFTs, governance tokens, or significant personal funds

### Minimum separation

```
Personal wallet (cold, hardware)
    ↓ send only fill capital
Solver wallet (hot, solver-main signs)
    ↓ earns protocol fees
Treasury / sweep address (cold)
```

The rebalancer (`portfolio-sidecar`) automates sweeping surplus from fill chains back to the home chain (Base). Configure it and let it run rather than manually moving funds.

### Monitor your wallet

The outcome dashboard (SQLite + web UI) tracks every fill. Run it:

```bash
# outcomes DB is at ./outcomes/wallet_mainnet.sqlite
# The solver exposes a dashboard at http://localhost:8082 when running
```

Set up on-chain alerts (e.g., Tenderly, Chainlink Automation, or a simple cron + curl) to notify you if the wallet balance drops below a threshold.

---

## 10. Colosseum / Frontier Specific Notes

### What counts as a valid hackathon fill

For demo purposes under both tracks:
- Fill must be broadcast to mainnet (not testnet)
- Fill tx must be confirmed and recorded in the outcome SQLite
- The intent must have originated from the genome stream (not self-created)
- `DRY_RUN=false` must have been set at the time of broadcast

### Testnet mode (safe for development)

Use `run-testnet.sh` for development. It points to Base Sepolia and Solana Devnet and uses a separate testnet wallet. Testnet fills do not count for hackathon scoring but are the correct place to verify your setup before going live.

### Submitting fills as evidence

The outcome SQLite at `./outcomes/mainnet_*.sqlite` contains:
- `intent_id` — genome intent identifier
- `protocol` — which bridge was filled
- `fill_tx_hash` — on-chain tx hash (verifiable on Basescan/Arbiscan)
- `fill_chain_id` — which chain the fill landed on
- `profit_usd` — net profit after gas and protocol fees
- `claim_tx_hash` — fee claim transaction (populated after claim)

Export as JSON for submission:
```bash
sqlite3 ./outcomes/mainnet_*.sqlite ".mode json" "SELECT * FROM fills ORDER BY created_at DESC LIMIT 20"
```

---

## 11. Security Checklist Before Going Live

Run through this list before setting `DRY_RUN=false`:

- [ ] Solver EVM key stored in macOS keychain (`mamba-messiah-key`) or secrets manager — not in `.env` or shell history
- [ ] Solver Solana key stored in macOS keychain (`mamba-messiah-solana-key`) or secrets manager
- [ ] Solver wallet is a dedicated address — not shared with any personal holdings
- [ ] Wallet funded with only the capital you are willing to lose
- [ ] `MAX_NOTIONAL_USD` set to a safe value ($5–$20 for first run)
- [ ] `DRY_RUN=true` run completed and output reviewed — no unexpected behavior
- [ ] `PROTOCOL_FILTER` and `DST_CHAIN_FILTER` set to chains/protocols you have capital for
- [ ] No `.env` file with secrets in the repo directory
- [ ] `git status` shows no secrets-containing files staged or untracked
- [ ] `RUST_LOG` is not set to `trace`
- [ ] You know how to kill the solver (`pkill -SIGTERM taifoon-solver`)
- [ ] You have a sweep/treasury address to receive profits separate from the hot wallet

---

## 12. Reporting Security Issues

Do not open public GitHub issues for security vulnerabilities. Contact the maintainer directly at the address on the GitHub profile or via the hackathon Discord DM channel marked `#security`.

Include:
- Affected component (file + line if known)
- Description of the issue
- Steps to reproduce (if applicable)
- Whether you believe it is exploitable in the current deployment
