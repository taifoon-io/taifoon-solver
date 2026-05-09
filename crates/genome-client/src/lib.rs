use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tracing::{error, info, warn};

/// Genome event from DA API SSE stream.
///
/// Legacy field names (`token`, `amount`, `timestamp`, `ref`) are honored via
/// serde aliases so older fixtures still parse — but ONLY when the canonical
/// keys (`src_token`, `input_amount`, `ts`, `ref_hash`) are absent. Real-shaped
/// fixtures from the spinner genome_encoder include both the legacy and
/// canonical keys; for those, deserialize via [`GenomeEvent::from_json_value`]
/// (or [`GenomeEvent::from_json_str`]) which strips the legacy duplicates
/// before invoking serde. The plain `serde::Deserialize` impl is preserved
/// for the in-process SSE consumer where each event carries only one key set.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenomeEvent {
    /// Full address (e.g., "T:1745678/proto:lifi_v2/deposit:1:0xabc123")
    #[serde(default, alias = "address")]
    pub addr: String,
    /// Entity type (e.g., "proto", "order")
    pub entity: String,
    /// Protocol ID or order ID (e.g., "lifi_v2" or full order_id)
    #[serde(default)]
    pub id: Option<String>,
    /// Action (e.g., "deposit", "placed", "executed")
    pub action: String,
    /// Chain ID
    pub chain_id: Option<u64>,
    /// Transaction reference
    #[serde(default, rename = "ref_hash", alias = "ref")]
    pub reference: Option<String>,
    /// Source chain (for cross-chain intents)
    pub src_chain: Option<u64>,
    /// Destination chain (for cross-chain intents)
    pub dst_chain: Option<u64>,
    /// Depositor address
    pub depositor: Option<String>,
    /// Recipient address (optional)
    pub recipient: Option<String>,
    /// Source token address
    #[serde(default, alias = "token")]
    pub src_token: Option<String>,
    /// Destination token address
    pub dst_token: Option<String>,
    /// Amount (as string to preserve precision)
    #[serde(default, alias = "amount")]
    pub input_amount: Option<String>,
    /// Timestamp
    #[serde(default, alias = "timestamp")]
    pub ts: Option<u64>,
    /// Genome snapshot batch id (unix timestamp, top-level field on every event).
    #[serde(default)]
    pub batch_id: Option<u64>,
    /// Protocol name (for order entities)
    pub protocol: Option<String>,
    /// Order ID (for order entities)
    pub order_id: Option<String>,

    // ── Protocol-specific fields the executor needs (B.1) ─────────────────────
    /// Negotiated output amount (Across: V3FundsDeposited.outputAmount;
    /// LiFi: minAmountOut). String to preserve precision.
    #[serde(default, alias = "min_amount_out")]
    pub output_amount: Option<String>,
    /// Across V3 depositId (int64 in the deployed adapter).
    #[serde(default)]
    pub deposit_id: Option<i64>,
    /// deBridge maker order nonce (uint64).
    #[serde(default)]
    pub maker_order_nonce: Option<u64>,
    /// deBridge give-amount in source-token base units (string for precision).
    #[serde(default)]
    pub give_amount: Option<String>,
    /// deBridge take-amount in destination-token base units.
    #[serde(default)]
    pub take_amount: Option<String>,

    // ── Protocol-specific fields the executor needs (B.2) ─────────────────────
    /// Mayan Swift order_id (32-byte hex), the on-chain order hash.
    #[serde(default)]
    pub mayan_order_id: Option<String>,
    /// Mayan Swift destination-chain Wormhole id (e.g. 30 = Base).
    #[serde(default)]
    pub swift_dest_chain_wormhole_id: Option<u16>,
    /// Mayan trader address (the depositor on src chain).
    #[serde(default)]
    pub trader: Option<String>,
    /// Mayan unix-seconds deadline after which the order can no longer be filled.
    #[serde(default)]
    pub deadline: Option<u64>,
    /// LiFi quote id (32-byte hex) — distinguishes a single quote within a route.
    #[serde(default)]
    pub lifi_quote_id: Option<String>,
    /// LiFi transactionId (the bytes32 carried by `LiFiTransferStarted`).
    #[serde(default)]
    pub lifi_transaction_id: Option<String>,
    /// Underlying bridge LiFi routed to ("across" | "stargate" | "mayan" | ...).
    /// The meta-router uses this to dispatch to the matching adapter.
    #[serde(default)]
    pub bridge: Option<String>,
    /// LiFi tool name — usually equal to `bridge` but kept distinct so we can
    /// detect mismatches and fall back to RouteNotImplemented gracefully.
    #[serde(default)]
    pub tool: Option<String>,

    // ── Mayan-Solana fields the Solana adapter needs (B.3) ────────────────────
    /// Mayan Swift Solana program id (base58, e.g. BLZRi6frs4X4DNLw56V4EXai1b6QVESN1BhHBTYM9VcY).
    #[serde(default)]
    pub swift_program_id: Option<String>,
    /// State PDA holding the on-chain order metadata (base58).
    #[serde(default)]
    pub state_account: Option<String>,
    /// Vault PDA that escrows the source-side tokens for this order (base58).
    #[serde(default)]
    pub vault_account: Option<String>,
    /// Mayan-side estimate of compute units needed for `fulfill` (advisory, ~240k).
    #[serde(default)]
    pub compute_units_estimate: Option<u64>,
    /// True when the source chain is Solana (i.e. the order was opened on Solana).
    /// Lets the executor pick the SVM path without re-checking chain ids.
    #[serde(default)]
    pub is_solana_source: Option<bool>,

    // ── Across V3 relay parameters needed for fillV3Relay ─────────────────────
    /// Across V3 fillDeadline (unix seconds). Must match the on-chain deposit.
    #[serde(default)]
    pub fill_deadline: Option<u32>,
    /// Across V3 exclusivityDeadline (unix seconds, 0 = no exclusive relayer).
    #[serde(default)]
    pub exclusivity_deadline: Option<u32>,
    /// Across V3 exclusiveRelayer address (0x0 = no exclusive relayer).
    #[serde(default)]
    pub exclusive_relayer: Option<String>,
    /// Across V3 message (hex bytes, "0x" = empty).
    #[serde(default)]
    pub message: Option<String>,
}

impl GenomeEvent {
    /// Deserialize from a JSON value while tolerating fixtures that carry
    /// both the canonical key and its legacy alias (e.g. both `src_token`
    /// and `token`). When both are present, the canonical key wins; the
    /// legacy duplicate is stripped before serde sees it.
    pub fn from_json_value(mut v: serde_json::Value) -> Result<Self> {
        if let Some(obj) = v.as_object_mut() {
            // (canonical, legacy) pairs — strip legacy if canonical is present.
            const PAIRS: &[(&str, &str)] = &[
                ("src_token", "token"),
                ("input_amount", "amount"),
                ("ts", "timestamp"),
                ("ref_hash", "ref"),
                // Mayan/LiFi fixtures carry both `output_amount` (canonical) and
                // `min_amount_out` (legacy alias). Strip the legacy key so serde
                // doesn't see it as a duplicate field.
                ("output_amount", "min_amount_out"),
            ];
            for (canonical, legacy) in PAIRS {
                if obj.contains_key(*canonical) && obj.contains_key(*legacy) {
                    obj.remove(*legacy);
                }
            }
        }
        serde_json::from_value(v).context("deserialize GenomeEvent")
    }

    /// Convenience: deserialize from a JSON string with the same legacy-key
    /// tolerance as [`from_json_value`].
    pub fn from_json_str(s: &str) -> Result<Self> {
        let v: serde_json::Value = serde_json::from_str(s).context("parse JSON")?;
        Self::from_json_value(v)
    }
}

/// Simplified intent structure for solver
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Intent {
    /// Unique intent ID
    pub id: String,
    /// Protocol name (e.g., "lifi_v2", "stargate_v2")
    pub protocol: String,
    /// Source chain ID
    pub src_chain: u64,
    /// Destination chain ID
    pub dst_chain: u64,
    /// Token address on source chain
    pub src_token: String,
    /// Token address on destination chain (might be same as src_token)
    pub dst_token: String,
    /// Amount to transfer (in token's smallest unit)
    pub amount: String,
    /// Depositor address
    pub depositor: String,
    /// Recipient address
    pub recipient: String,
    /// Transaction hash on source chain
    pub tx_hash: String,
    /// Timestamp when detected
    pub detected_at: u64,

    // ── Protocol-specific fields plumbed through to executor (B.1) ────────────
    /// Negotiated output amount on the destination chain (string base units).
    #[serde(default)]
    pub output_amount: Option<String>,
    /// Across V3 depositId.
    #[serde(default)]
    pub deposit_id: Option<i64>,
    /// deBridge maker order nonce.
    #[serde(default)]
    pub maker_order_nonce: Option<u64>,
    /// deBridge give-amount.
    #[serde(default)]
    pub give_amount: Option<String>,
    /// deBridge take-amount.
    #[serde(default)]
    pub take_amount: Option<String>,
    /// Order ID (deBridge orderId, Mayan order_id, etc.) preserved alongside `id`.
    #[serde(default)]
    pub order_id: Option<String>,
    /// deBridge Order.givePatchAuthoritySrc (hex bytes, usually empty).
    #[serde(default)]
    pub dln_give_patch_authority_src: Option<String>,
    /// deBridge Order.orderAuthorityAddressDst (hex bytes, usually empty).
    #[serde(default)]
    pub dln_order_authority_address_dst: Option<String>,
    /// deBridge Order.allowedTakerDst (hex bytes, empty = any taker allowed).
    #[serde(default)]
    pub dln_allowed_taker_dst: Option<String>,
    /// deBridge Order.allowedCancelBeneficiarySrc (hex bytes, usually empty).
    #[serde(default)]
    pub dln_allowed_cancel_beneficiary_src: Option<String>,
    /// deBridge Order.externalCall (hex bytes, usually empty).
    #[serde(default)]
    pub dln_external_call: Option<String>,

    // ── Protocol-specific fields plumbed through to executor (B.2) ────────────
    /// Mayan Swift order hash (32-byte hex).
    #[serde(default)]
    pub mayan_order_id: Option<String>,
    /// Mayan Swift destination-chain Wormhole id.
    #[serde(default)]
    pub swift_dest_chain_wormhole_id: Option<u16>,
    /// Mayan trader (depositor on src chain).
    #[serde(default)]
    pub trader: Option<String>,
    /// Mayan deadline (unix-seconds).
    #[serde(default)]
    pub deadline: Option<u64>,
    /// LiFi quote id (32-byte hex).
    #[serde(default)]
    pub lifi_quote_id: Option<String>,
    /// LiFi transactionId (32-byte hex carried by `LiFiTransferStarted`).
    #[serde(default)]
    pub lifi_transaction_id: Option<String>,
    /// Underlying bridge for LiFi meta-routing ("across" | "stargate" | "mayan" | ...).
    #[serde(default)]
    pub bridge: Option<String>,
    /// LiFi tool name.
    #[serde(default)]
    pub tool: Option<String>,

    // ── Mayan-Solana fields plumbed through to executor (B.3) ─────────────────
    /// Mayan Swift Solana program id (base58).
    #[serde(default)]
    pub swift_program_id: Option<String>,
    /// State PDA (base58).
    #[serde(default)]
    pub state_account: Option<String>,
    /// Vault PDA (base58).
    #[serde(default)]
    pub vault_account: Option<String>,
    /// Compute-unit estimate from the genome event.
    #[serde(default)]
    pub compute_units_estimate: Option<u64>,
    /// True when the source chain is Solana.
    #[serde(default)]
    pub is_solana_source: Option<bool>,

    // ── Across V3 relay parameters ─────────────────────────────────────────────
    /// fillDeadline from the Across V3 deposit event (unix seconds).
    #[serde(default)]
    pub fill_deadline: Option<u32>,
    /// exclusivityDeadline from the Across V3 deposit event (0 = none).
    #[serde(default)]
    pub exclusivity_deadline: Option<u32>,
    /// exclusiveRelayer from the Across V3 deposit event ("0x0..." = none).
    #[serde(default)]
    pub exclusive_relayer: Option<String>,
    /// message from the Across V3 deposit event (hex, "0x" = empty).
    #[serde(default)]
    pub message: Option<String>,
    /// Genome snapshot batch_id — used as batchId in executeVerifiedCall V1.
    #[serde(default)]
    pub batch_id: Option<u64>,

    // ── Mayan Swift V2 order fields decoded from source tx ────────────────────
    /// 32-byte random salt included in the Mayan Swift V2 OrderParams.
    /// Decoded from the `createOrderWithToken` calldata of the source tx.
    /// Required to reconstruct the exact OrderParams for `fulfillOrder`/`fulfillSimple`.
    #[serde(default)]
    pub mayan_random: Option<String>,
    /// Mayan Swift auction mode: 0=no-auction (fulfillSimple, no VAA needed),
    /// 2=auction (fulfillOrder, requires auction VAA from chain 42069).
    #[serde(default)]
    pub mayan_auction_mode: Option<u8>,
    /// Mayan Swift cancel relayer fee (raw u64, from OrderParams).
    #[serde(default)]
    pub mayan_cancel_fee: Option<u64>,
    /// Mayan Swift refund relayer fee (raw u64, from OrderParams).
    #[serde(default)]
    pub mayan_refund_fee: Option<u64>,
    /// Mayan Swift referrer address (bytes32 hex, from OrderParams).
    #[serde(default)]
    pub mayan_referrer_addr: Option<String>,
    /// Mayan Swift referrer bps (from OrderParams).
    #[serde(default)]
    pub mayan_referrer_bps: Option<u8>,
    /// Mayan Swift gasDrop amount (raw u64, from OrderParams).
    #[serde(default)]
    pub mayan_gas_drop: Option<u64>,

    // ── DLN Solana destination fields ─────────────────────────────────────────
    /// True when the destination chain is Solana (chain_id = 100_000_001).
    /// Set by the DeBridge pollers when they pass a Solana-destination order through.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_solana_destination: Option<bool>,
}

impl Intent {
    /// Parse intent from genome event
    pub fn from_genome_event(event: GenomeEvent) -> Result<Self> {
        let src_chain = event
            .src_chain
            .context("Missing src_chain in genome event")?;
        let dst_chain = event
            .dst_chain
            .context("Missing dst_chain in genome event")?;

        // Support both input_amount (new) and amount (old) - with fallback to skip intent
        let protocol_name = event.protocol.as_ref().or(event.id.as_ref()).map(|s| s.as_str()).unwrap_or("unknown");

        let amount_raw = event.input_amount
            .clone()
            .or_else(|| {
                warn!("⚠️  Protocol '{}' missing 'input_amount', this intent will be skipped", protocol_name);
                None
            })
            .context(format!("Missing input_amount field for protocol '{}' - genome stream data incomplete", protocol_name))?;

        // Guard against sentinel values that exceed u128::MAX.
        // LiFi proto/deposit events occasionally set input_amount to a bytes32/address field,
        // producing numbers > 2^128 that crash profit_calc with parse-overflow errors.
        if amount_raw.parse::<u128>().is_err() {
            anyhow::bail!(
                "Protocol '{}' input_amount '{}...' overflows u128 — skipping (likely address/bytes32 misread as amount)",
                protocol_name, &amount_raw[..amount_raw.len().min(40)]
            );
        }
        let amount = amount_raw;

        let depositor = event.depositor.clone()
            // deBridge order events use 'maker' instead of 'depositor' — fall back.
            .or_else(|| event.trader.clone())
            .context("Missing depositor in genome event")?;

        // Recipient may be optional in some protocols
        let recipient = event.recipient.clone().unwrap_or_else(|| depositor.clone());

        // Use ref_hash as tx hash; for LiFi events the Diamond tx appears in the genome entity
        // addr (e.g. "T:1745678/proto:lifi_v2/deposit:1:0x8f402c...") but not in ref_hash.
        // Extract it as a fallback before synthesizing a random hash.
        let addr_tx_hash = if event.addr.contains("0x") {
            event.addr.split("0x").last()
                .filter(|s| s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit()))
                .map(|s| format!("0x{}", s))
        } else {
            None
        };
        let tx_hash = event
            .reference.clone()
            .or_else(|| event.order_id.clone())
            .or(addr_tx_hash)
            .unwrap_or_else(|| {
                // Generate synthetic tx_hash for protocols that don't provide one (e.g., Li.Fi)
                use std::collections::hash_map::DefaultHasher;
                use std::hash::{Hash, Hasher};

                let mut hasher = DefaultHasher::new();
                if let Some(ref dep) = event.depositor {
                    dep.hash(&mut hasher);
                }
                event.src_chain.hash(&mut hasher);
                event.dst_chain.hash(&mut hasher);
                if let Some(ref amt) = event.input_amount {
                    amt.hash(&mut hasher);
                }
                event.ts.hash(&mut hasher);

                let synthetic_hash = format!("synthetic_{:x}", hasher.finish());
                warn!("⚠️  Generating synthetic tx_hash for protocol {:?} (missing reference): {}",
                      event.protocol.as_ref().or(event.id.as_ref()), synthetic_hash);
                synthetic_hash
            });

        // Support both src_token (new) and token (old) - with intelligent fallback
        let src_token = event.src_token
            .clone()
            .or_else(|| {
                // Fallback: infer native token (0x0) for the source chain
                warn!("⚠️  Protocol '{}' missing 'src_token', inferring native token address", protocol_name);
                Some("0x0000000000000000000000000000000000000000".to_string())
            })
            .context(format!("Missing src_token field for protocol '{}' - genome stream data incomplete", protocol_name))?;

        let dst_token = event.dst_token.clone().unwrap_or_else(|| src_token.clone());

        // Protocol: use protocol field if available, otherwise use id
        let protocol = event.protocol.clone().or_else(|| event.id.clone())
            .context("Missing protocol/id in genome event")?;

        // Generate unique intent ID from protocol + tx hash
        let id = format!("{}:{}", protocol, tx_hash);

        Ok(Intent {
            id,
            protocol,
            src_chain,
            dst_chain,
            src_token,
            dst_token,
            amount,
            depositor,
            recipient,
            tx_hash,
            detected_at: event.ts.unwrap_or_else(|| {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64 / 1000
            }),
            output_amount: event.output_amount,
            deposit_id: event.deposit_id,
            maker_order_nonce: event.maker_order_nonce,
            give_amount: event.give_amount,
            take_amount: event.take_amount,
            order_id: event.order_id,
            dln_give_patch_authority_src: None,
            dln_order_authority_address_dst: None,
            dln_allowed_taker_dst: None,
            dln_allowed_cancel_beneficiary_src: None,
            dln_external_call: None,
            mayan_order_id: event.mayan_order_id,
            swift_dest_chain_wormhole_id: event.swift_dest_chain_wormhole_id,
            trader: event.trader,
            deadline: event.deadline,
            lifi_quote_id: event.lifi_quote_id,
            lifi_transaction_id: event.lifi_transaction_id,
            bridge: event.bridge,
            tool: event.tool,
            swift_program_id: event.swift_program_id,
            state_account: event.state_account,
            vault_account: event.vault_account,
            compute_units_estimate: event.compute_units_estimate,
            is_solana_source: event.is_solana_source,
            fill_deadline: event.fill_deadline,
            exclusivity_deadline: event.exclusivity_deadline,
            exclusive_relayer: event.exclusive_relayer,
            message: event.message,
            batch_id: event.batch_id,
            mayan_random: None,
            mayan_auction_mode: None,
            mayan_cancel_fee: None,
            mayan_refund_fee: None,
            mayan_referrer_addr: None,
            mayan_referrer_bps: None,
            mayan_gas_drop: None,
            is_solana_destination: None,
        })
    }
}

/// Genome SSE client
pub struct GenomeClient {
    /// SSE endpoint URL
    sse_url: String,
    /// HTTP client
    client: reqwest::Client,
}

/// Across REST poller — fills the gap when the genome SSE stream does not emit
/// `entity: "proto"` deposit events. Polls the Across V3 deposits API every
/// `poll_interval_secs` seconds for each destination chain and synthesizes
/// `Intent` objects directly from the API response.
///
/// Spawned as a background task by `GenomeClient::subscribe_with_pollers`.
pub struct AcrossPoller {
    /// Destination chain IDs to poll (e.g. [8453, 10, 42161]).
    pub dst_chains: Vec<u64>,
    /// Poll interval in seconds.
    pub poll_interval_secs: u64,
    /// Max deposits per chain per poll.
    pub limit: usize,
    /// Solver EVM address (lowercase). When set, deposits where exclusiveRelayer == this
    /// address are processed even while still in their exclusivity window.
    pub solver_address: Option<String>,
}

impl AcrossPoller {
    pub fn default_mainnet() -> Self {
        Self {
            // Only chains where solver has funded liquidity:
            //   10=Optimism (0.22 USDC), 8453=Base (0.97 USDC), 42161=Arbitrum (0.03 USDC).
            // 3 chains × 20s = 60s/sweep + 30s rest = ~90s cycle. No 429 risk.
            // limit=50 ensures we see orders deeper in the unfilled queue — with the
            // efficient Across market the first 20 are often already exclusive-locked.
            dst_chains: vec![10, 8453, 42161],
            poll_interval_secs: 30,
            limit: 50,
            solver_address: None,
        }
    }

    /// Run forever, sending fillable Across intents to `intent_tx`.
    /// Tracks seen depositIds in a local set to avoid re-emitting.
    pub async fn run(self, intent_tx: tokio::sync::mpsc::Sender<Intent>) {
        use std::collections::HashSet;
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(8))
            .user_agent("Mozilla/5.0 (compatible; taifoon-solver/1.0)")
            .build()
            .unwrap_or_default();
        let mut seen: HashSet<i64> = HashSet::new();
        // Cap the seen set at 10 000 entries — once filled deposits drop off the
        // Across API they will never re-appear, so the entries are stale. Evict
        // the 1 000 smallest (numerically oldest) IDs when the cap is hit.
        const SEEN_CAP: usize = 10_000;
        const SEEN_EVICT: usize = 1_000;
        // Backoff state: incremented on 429, reset on success.
        let mut consecutive_429s: u32 = 0;

        loop {
            for &dst_chain in &self.dst_chains {
                // 20s inter-chain sleep: 3 chains × 20s = 60s/sweep, well under Cloudflare
                // rate limit threshold. Manual curl calls can trigger temporary bans.
                tokio::time::sleep(std::time::Duration::from_secs(20)).await;
                let url = format!(
                    "https://app.across.to/api/deposits?status=unfilled&destinationChainId={}&limit={}",
                    dst_chain, self.limit
                );
                let resp = match client.get(&url).send().await {
                    Ok(r) => r,
                    Err(e) => { tracing::warn!("AcrossPoller chain={} request error: {}", dst_chain, e); continue; }
                };
                if resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
                    consecutive_429s += 1;
                    let backoff = std::cmp::min(30 * consecutive_429s, 300);
                    tracing::warn!("AcrossPoller chain={} rate-limited (429), backing off {}s", dst_chain, backoff);
                    tokio::time::sleep(std::time::Duration::from_secs(backoff as u64)).await;
                    continue;
                }
                if !resp.status().is_success() {
                    tracing::warn!("AcrossPoller chain={} HTTP {}: skipping", dst_chain, resp.status());
                    continue;
                }
                consecutive_429s = 0;
                let deps: Vec<serde_json::Value> = match resp.json().await {
                    Ok(d) => d,
                    Err(e) => { tracing::warn!("AcrossPoller chain={} parse error: {}", dst_chain, e); continue; }
                };
                tracing::info!("AcrossPoller chain={} polled: {} unfilled deposits", dst_chain, deps.len());

                let now_secs = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as i64;

                for dep in deps {
                    // depositId may be a JSON string ("5619364") or integer depending on API version
                    let dep_id = match dep.get("depositId") {
                        Some(v) => {
                            if let Some(n) = v.as_i64() { n }
                            else if let Some(s) = v.as_str() {
                                match s.parse::<i64>() { Ok(n) => n, Err(_) => continue }
                            } else { continue }
                        }
                        None => continue,
                    };
                    // Exclusivity check: skip (but don't dedup) if still in exclusive window.
                    // Dedup after exclusivity so an expiring exclusive deposit can be filled later.
                    let excl = dep.get("exclusiveRelayer")
                        .and_then(|v| v.as_str())
                        .unwrap_or("0x0000000000000000000000000000000000000000");
                    let is_exclusive = !excl.is_empty()
                        && excl != "0x0000000000000000000000000000000000000000";
                    if is_exclusive {
                        let excl_deadline = dep.get("exclusivityDeadline")
                            .and_then(|v| v.as_i64().or_else(|| v.as_str().and_then(|s| chrono_unix_from_iso(s))))
                            .unwrap_or(0);
                        // Allow if we ARE the exclusive relayer — only skip if someone else is.
                        let we_are_exclusive = self.solver_address.as_deref()
                            .map(|sa| sa.to_lowercase() == excl.to_lowercase())
                            .unwrap_or(false);
                        if excl_deadline > now_secs && !we_are_exclusive {
                            continue; // still exclusive to another relayer — retry when expired
                        }
                    }

                    // Fill deadline: skip if < 60s remaining.
                    // Across API may return fillDeadline as either a JSON integer or a string.
                    let fill_deadline_unix = dep.get("fillDeadline")
                        .and_then(|v| {
                            v.as_i64()
                                .or_else(|| v.as_str().and_then(|s| chrono_unix_from_iso(s)))
                        })
                        .unwrap_or(0);
                    if fill_deadline_unix > 0 && fill_deadline_unix <= now_secs + 60 {
                        continue;
                    }

                    // Dedup: only after deadline/exclusivity checks so we don't permanently
                    // block deposits that were skipped due to a temporary condition.
                    // Evict oldest (smallest) deposit IDs when cap is reached.
                    if seen.len() >= SEEN_CAP {
                        let mut ids: Vec<i64> = seen.iter().copied().collect();
                        ids.sort_unstable();
                        for id in ids.into_iter().take(SEEN_EVICT) {
                            seen.remove(&id);
                        }
                    }
                    if !seen.insert(dep_id) {
                        continue;
                    }

                    let intent = across_deposit_to_intent(&dep, dst_chain, dep_id, fill_deadline_unix);
                    info!("📡 AcrossPoller: depositId={} {}→{} outAmt={}",
                        dep_id, intent.src_chain, intent.dst_chain, intent.amount);

                    if intent_tx.send(intent).await.is_err() {
                        return; // receiver dropped
                    }
                }

            }

            tokio::time::sleep(std::time::Duration::from_secs(self.poll_interval_secs)).await;
        }
    }
}

fn chrono_unix_from_iso(s: &str) -> Option<i64> {
    // Handles both ISO 8601 ("2026-04-28T12:00:00Z") and raw unix int strings ("1745678400")
    if let Ok(n) = s.parse::<i64>() {
        return Some(n);
    }
    // Try ISO 8601
    let clean = s.replace('Z', "+00:00");
    // Manual parse: "2026-04-28T12:00:00+00:00"
    if clean.len() >= 19 {
        let date_part = &clean[..10]; // "2026-04-28"
        let time_part = &clean[11..19]; // "12:00:00"
        let ymd: Vec<u32> = date_part.split('-').filter_map(|x| x.parse().ok()).collect();
        let hms: Vec<u32> = time_part.split(':').filter_map(|x| x.parse().ok()).collect();
        if ymd.len() == 3 && hms.len() == 3 {
            // Compute unix timestamp manually (accurate for dates ~2020-2040)
            let year = ymd[0] as i64;
            let month = ymd[1] as i64;
            let day = ymd[2] as i64;
            // Days since epoch
            let y = if month <= 2 { year - 1 } else { year };
            let m = if month <= 2 { month + 9 } else { month - 3 };
            let jdn = 365 * y + y / 4 - y / 100 + y / 400
                + (153 * m + 2) / 5 + day - 719469;
            let secs = jdn * 86400
                + hms[0] as i64 * 3600
                + hms[1] as i64 * 60
                + hms[2] as i64;
            return Some(secs);
        }
    }
    None
}

fn across_deposit_to_intent(
    dep: &serde_json::Value,
    dst_chain: u64,
    dep_id: i64,
    fill_deadline_unix: i64,
) -> Intent {
    let src_chain = dep.get("originChainId").and_then(|v| v.as_u64()).unwrap_or_else(|| {
        tracing::warn!("across_deposit: missing originChainId for dep_id={dep_id}, defaulting to mainnet (1)");
        1
    });
    let depositor = dep.get("depositor").and_then(|v| v.as_str()).unwrap_or("0x0").to_string();
    let recipient = dep.get("recipient").and_then(|v| v.as_str())
        .unwrap_or(&depositor).to_string();
    let input_token = dep.get("inputToken").and_then(|v| v.as_str()).unwrap_or("0x0").to_string();
    let output_token = dep.get("outputToken").and_then(|v| v.as_str()).unwrap_or("0x0").to_string();
    // inputAmount / outputAmount arrive as JSON strings in the Across API.
    // Fall back through f64→u128 (not u64) to handle large WETH amounts
    // (> 18.4 ETH in wei overflows u64::MAX → as_u64() returns None silently).
    let input_amount = dep.get("inputAmount")
        .and_then(|v| v.as_str().map(|s| s.to_string())
            .or_else(|| v.as_f64().map(|n| (n as u128).to_string())))
        .unwrap_or_else(|| "0".to_string());
    let output_amount = dep.get("outputAmount")
        .and_then(|v| v.as_str().map(|s| s.to_string())
            .or_else(|| v.as_f64().map(|n| (n as u128).to_string())))
        .unwrap_or_else(|| "0".to_string());
    let tx_hash = dep.get("depositTxHash").and_then(|v| v.as_str())
        .or_else(|| dep.get("txHash").and_then(|v| v.as_str()))
        .unwrap_or("0x").to_string();
    let excl_relayer = dep.get("exclusiveRelayer").and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let excl_deadline = dep.get("exclusivityDeadline")
        .and_then(|v| v.as_i64().or_else(|| v.as_str().and_then(|s| chrono_unix_from_iso(s))))
        .map(|v| v as u32);
    let message = dep.get("message").and_then(|v| v.as_str())
        .map(|s| s.to_string());

    Intent {
        id: format!("across_v3:dep:{}", dep_id),
        protocol: "across_v3".to_string(),
        src_chain,
        dst_chain,
        src_token: input_token.clone(),
        dst_token: output_token.clone(),
        amount: input_amount.clone(),
        depositor: depositor.clone(),
        recipient: recipient.clone(),
        tx_hash,
        output_amount: Some(output_amount),
        deposit_id: Some(dep_id),
        fill_deadline: if fill_deadline_unix > 0 { Some(fill_deadline_unix as u32) } else { None },
        exclusivity_deadline: excl_deadline,
        exclusive_relayer: excl_relayer,
        message,
        detected_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        ..Default::default()
    }
}

/// deBridge DLN on-chain `OrderCreated` log poller.
///
/// The deBridge public API has no open-order feed — it requires a known orderId
/// to query. This poller uses `eth_getLogs` to scan the DlnSource contract for
/// `OrderCreated` events across supported chains, then synthesizes `Intent`s
/// from the decoded log data. Uses block-range sliding windows to avoid RPC
/// `eth_getLogs` limits (some RPCs cap at 2000 blocks per call).
pub struct DeBridgePoller {
    /// (chain_id, rpc_url) pairs to monitor for new deBridge orders.
    pub chains: Vec<(u64, String)>,
    /// Poll interval in seconds.
    pub poll_interval_secs: u64,
    /// Blocks per `eth_getLogs` batch (2000 is safe for most RPCs).
    pub blocks_per_batch: u64,
}

/// DlnSource contract address (same on all EVM chains) — this is where users create orders
/// and where the OrderCreated event is emitted. Solvers poll this for new fill opportunities.
/// Different from DlnDestination (0xE7351Fd...) which is where solvers call fulfillOrder.
const DLN_SOURCE_ADDRESS: &str = "0xeF4fB24aD0916217251F553c0596F8Edc630EB66";
/// keccak256("CreatedOrder(...)") — emitted by DlnSource on every new cross-chain order.
const ORDER_CREATED_TOPIC: &str = "0xfc8703fd57380f9dd234a89dce51333782d49c5902f307b02f03e014d18fe471";

impl DeBridgePoller {
    /// Build a default poller for mainnet EVM chains that have a DlnSource.
    ///
    /// RPC URLs can be overridden via env vars:
    ///   DEBRIDGE_RPC_42161, DEBRIDGE_RPC_8453, DEBRIDGE_RPC_10,
    ///   DEBRIDGE_RPC_137, DEBRIDGE_RPC_56, DEBRIDGE_RPC_1
    pub fn default_mainnet() -> Self {
        let rpc = |chain_id: u64, fallback: &str| -> String {
            std::env::var(format!("DEBRIDGE_RPC_{}", chain_id))
                .unwrap_or_else(|_| fallback.to_string())
        };
        Self {
            chains: vec![
                (42161,  rpc(42161,  "https://arb1.arbitrum.io/rpc")),
                (8453,   rpc(8453,   "https://mainnet.base.org")),
                (10,     rpc(10,     "https://mainnet.optimism.io")),
                (137,    rpc(137,    "https://polygon-rpc.com")),
                (56,     rpc(56,     "https://bsc-dataseed.binance.org")),
                (59144,  "https://rpc.linea.build".into()),
                (1,      rpc(1,      "https://eth.llamarpc.com")),
                (534352, "https://rpc.scroll.io".into()),
                (57073,  "https://rpc-gel.inkonchain.com".into()),
                (34443,  "https://mainnet.mode.network".into()),
            ],
            poll_interval_secs: 12,
            blocks_per_batch: 2000,
        }
    }

    pub async fn run(self, intent_tx: tokio::sync::mpsc::Sender<Intent>) {
        use std::collections::HashMap;
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(8))
            .user_agent("Mozilla/5.0 (compatible; taifoon-solver/1.0)")
            .build()
            .unwrap_or_default();

        // Track the last-seen block per chain
        let mut last_block: HashMap<u64, u64> = HashMap::new();
        // Track emitted order_ids to avoid duplicates
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

        loop {
            for (chain_id, rpc) in &self.chains {
                // 1. Get current block number
                let current_block = match eth_block_number(&client, rpc).await {
                    Some(n) => n,
                    None => { continue; }
                };

                let from_block = last_block.get(chain_id)
                    .copied()
                    .unwrap_or(current_block.saturating_sub(self.blocks_per_batch))
                    .max(current_block.saturating_sub(self.blocks_per_batch * 5));
                let to_block = current_block;

                if from_block >= to_block {
                    last_block.insert(*chain_id, to_block);
                    continue;
                }

                // 2. eth_getLogs for OrderCreated on DlnSource
                let logs = match eth_get_logs(
                    &client, rpc, DLN_SOURCE_ADDRESS, ORDER_CREATED_TOPIC,
                    from_block, to_block,
                ).await {
                    Some(l) => l,
                    None => { continue; }
                };

                last_block.insert(*chain_id, to_block + 1);

                for log in logs {
                    if let Some(mut intent) = decode_dln_order_created_log(&log, *chain_id) {
                        if !seen.insert(intent.id.clone()) { continue; }
                        // Skip non-EVM destination chains — we have no fill path for them.
                        // Exception: Solana (100_000_001) is handled by DlnSolanaFiller.
                        if let Some(name) = debridge_non_evm_chain_name(intent.dst_chain) {
                            if intent.dst_chain != 100_000_001 {
                                info!("⏭️  DeBridgePoller skip non-EVM dst chain={} ({}) order={}",
                                    intent.dst_chain, name, intent.order_id.as_deref().unwrap_or("?"));
                                continue;
                            }
                            // Solana destination — allow through for DLN Solana fill path.
                            intent.is_solana_destination = Some(true);
                        }
                        // Skip orders where the take-token is not a known stablecoin/WETH —
                        // exotic tokens cause 18-decimal mis-pricing and we have no inventory.
                        // Exception: for Solana destinations the dst_token is a base58 SPL mint,
                        // not an EVM hex address, so skip the EVM token whitelist check.
                        if intent.dst_chain != 100_000_001 && !is_supported_fill_token(&intent.dst_token) {
                            info!("⏭️  DeBridgePoller skip exotic take_token={} order={}",
                                intent.dst_token, intent.order_id.as_deref().unwrap_or("?"));
                            continue;
                        }
                        info!("📡 DeBridgePoller chain={} orderId={} {}→{} give={}",
                            chain_id, intent.order_id.as_deref().unwrap_or("?"),
                            intent.src_chain, intent.dst_chain, intent.amount);
                        if intent_tx.send(intent).await.is_err() { return; }
                    }
                }

                tokio::time::sleep(std::time::Duration::from_millis(400)).await;
            }

            tokio::time::sleep(std::time::Duration::from_secs(self.poll_interval_secs)).await;
        }
    }
}

/// Returns a human-readable name for known deBridge non-EVM destination chain IDs,
/// or `None` for EVM chains we may be able to fill on.
/// Source: https://debridge.finance/chains (non-EVM registry as of 2026-05)
fn debridge_non_evm_chain_name(chain_id: u64) -> Option<&'static str> {
    match chain_id {
        100_000_001 => Some("Solana"),
        100_000_002 => Some("NEAR"),
        100_000_003 => Some("Tron"),
        100_000_004 => Some("Ton"),
        100_000_005 => Some("Aptos"),
        100_000_006 => Some("Sui"),
        100_000_007 => Some("Eclipse (Solana SVM)"),
        100_000_022 => Some("Neon EVM on Solana"),
        100_000_023 => Some("Sonic (Solana SVM)"),
        100_000_027 => Some("Solana (alt chain ID)"),
        100_000_030 => Some("Grass (Solana SVM)"),
        100_000_031 => Some("Svm-Unknown-31"),
        _ => None,
    }
}

/// Returns true when `addr` is a token the solver can actually fill:
/// USDC / USDT (any chain) or WETH / native-ETH.
fn is_supported_fill_token(addr: &str) -> bool {
    let lower = addr.to_lowercase();
    // Native ETH sentinel
    if lower == "0x0000000000000000000000000000000000000000" || lower == "native" {
        return true;
    }
    const SUPPORTED: &[&str] = &[
        // USDC (all chains)
        "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48", // ETH
        "0xaf88d065e77c8cc2239327c5edb3a432268e5831", // Arb native
        "0xff970a61a04b1ca14834a43f5de4533ebddb5cc8", // USDC.e Arb
        "0x833589fcd6edb6e08f4c7c32d4f71b54bda02913", // Base
        "0x0b2c639c533813f4aa9d7837caf62653d097ff85", // OP native
        "0x7f5c764cbc14f9669b88837ca1490cca17c31607", // USDC.e OP
        "0x3c499c542cef5e3811e1192ce70d8cc03d5c3359", // Polygon native
        "0x2791bca1f2de4661ed88a30c99a7a9449aa84174", // USDC.e Polygon
        "0x176211869ca2b568f2a7d4ee941e073a821ee1ff", // Linea
        "0x8ac76a51cc950d9822d68b83fe1ad97b32cd580d", // BNB USDC
        // USDT (all chains)
        "0xdac17f958d2ee523a2206206994597c13d831ec7", // ETH
        "0xfd086bc7cd5c481dcc9c85ebe478a1c0b69fcbb9", // Arb
        "0x94b008aa00579c1307b0ef2c499ad98a8ce58e58", // OP
        // BSC USDT (0x55d398) is 18-decimal — skip until decimal handling is confirmed
        "0xc2132d05d31c914a87c6611c10748aeb04b58e8f", // Polygon
        // WETH (all chains)
        "0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2", // ETH
        "0x82af49447d8a07e3bd95bd0d56f35241523fbab1", // Arb
        "0x4200000000000000000000000000000000000006", // Base / OP / Unichain / Ink / Mode
        "0xe5d7c2a44ffddf6b295a15c148167daaaf5cf34f", // Linea
        "0x2170ed0880ac9a755fd29b2688956bd959f933f8", // BSC WETH/WBTC
        "0x5300000000000000000000000000000000000004", // Scroll WETH
        "0x7ceb23fd6bc0add59e62ac25578270cff1b9f619", // Polygon WETH
        "0xbb4cdb9cbd36b01bd1cbaebf2de08d9173bc095c", // BNB WBNB
        "0x49d5c2bdffac6ce2bfdb6640f4f80f226bc10bab", // Avax WETH.e
        // USDC new chains
        "0x078d782b760474a361dda7ff6e249887ddf39eb0", // USDC Unichain
        "0x06efdbff2a14a7c8e15944d1f4a48f9f95f663a4", // Scroll USDC
        "0x2d270e6886d130d724215a266106e6832161eaed", // Ink USDC
        "0xd988097fb8612cc24eec14542bc03424c656005f", // Mode USDC.e
        "0x9c3c9283d3e44854697cd22d3faa240cfb032889", // Polygon zkEVM USDC
        "0xe0b7927c4af23765cb51314a0e0521a9645f0e2a", // Avax USDC.e (old)
        "0xb97ef9ef8734c71904d8002f8b6bc66dd9c48a6e", // Avax native USDC
        "0x8ac76a51cc950d9822d68b83fe1ad97b32cd580d", // already there (dup ok, filtered)
        // zkSync Era
        "0x1d17cbcf0d6d143135ae902365d2e5e2a16538d4", // USDC zkSync Era
        "0x5aea5775959fbc2557cc8789bc1bf90a239d9a91", // WETH zkSync Era
        // USDT new chains
        "0xfde4c96c8593536e31f229ea8f37b2ada2699bb2", // USDT Base
        "0xf55bec9cafdbe8730f096aa55dad6d22d44099df", // Scroll USDT
        "0x0200c29006150606b650577bbe7b6248f58470c1", // Ink USDT
        "0xf0f161fda2712db8b566946122a5af183995e2ed", // Mode USDT
        "0x9702230a8ea53601f5cd2dc00fdbc13d4df4a8c7", // Avax native USDT
        "0xc7198437980c041c805a1edcba50c1ce5db95118", // Avax USDT.e
    ];
    SUPPORTED.iter().any(|s| *s == lower.as_str())
}

async fn eth_block_number(client: &reqwest::Client, rpc: &str) -> Option<u64> {
    let body = serde_json::json!({
        "jsonrpc": "2.0", "id": 1,
        "method": "eth_blockNumber",
        "params": []
    });
    let hex = client.post(rpc).json(&body).send().await.ok()?
        .json::<serde_json::Value>().await.ok()?
        ["result"].as_str()?.to_string();
    u64::from_str_radix(hex.trim_start_matches("0x"), 16).ok()
}

async fn eth_get_logs(
    client: &reqwest::Client,
    rpc: &str,
    address: &str,
    topic0: &str,
    from_block: u64,
    to_block: u64,
) -> Option<Vec<serde_json::Value>> {
    let body = serde_json::json!({
        "jsonrpc": "2.0", "id": 1,
        "method": "eth_getLogs",
        "params": [{
            "address": address,
            "topics": [topic0],
            "fromBlock": format!("0x{:x}", from_block),
            "toBlock": format!("0x{:x}", to_block)
        }]
    });
    let resp = client.post(rpc).json(&body).send().await.ok()?
        .json::<serde_json::Value>().await.ok()?;
    resp["result"].as_array().cloned()
}

/// Decode a DlnSource `CreatedOrder` log into an `Intent`.
///
/// Verified against live Arbitrum logs. ABI layout (all slots are 32 bytes):
///
/// top-level args: (Order order, bytes32 orderId, bytes affiliateFeeOrderId, ...)
///   slot[0] = offset to Order tuple (= 224 bytes = 7 slots)
///   slot[1] = orderId (bytes32) — NOT indexed, in data
///   slot[2] = offset to affiliateFeeOrderId
///
/// Order struct at slot 7 (offsets inside Order are relative to Order start):
///   [+0] makerOrderNonce  uint64
///   [+1] offset to makerSrc bytes (relative)
///   [+2] giveChainId uint256
///   [+3] offset to giveTokenAddress bytes (relative)
///   [+4] giveAmount uint256
///   [+5] takeChainId uint256
///   [+6] offset to takeTokenAddress bytes (relative)
///   [+7] takeAmount uint256
///   [+8] offset to receiverDst bytes (relative)
///   ...
fn decode_dln_order_created_log(log: &serde_json::Value, src_chain_id: u64) -> Option<Intent> {
    let data_hex = log["data"].as_str()?.trim_start_matches("0x");
    if data_hex.len() < 128 { return None; }

    let slots: Vec<[u8; 32]> = data_hex
        .as_bytes()
        .chunks(64)
        .filter_map(|c| {
            let s = std::str::from_utf8(c).ok()?;
            let b = hex::decode(s).ok()?;
            if b.len() == 32 { let mut arr = [0u8; 32]; arr.copy_from_slice(&b); Some(arr) } else { None }
        })
        .collect();

    if slots.len() < 22 { return None; }

    // slot[0] = offset to Order in bytes — verified = 224 = slot 7
    let order_offset_bytes = u64::from_be_bytes(slots[0][24..32].try_into().ok()?) as usize;
    let os = order_offset_bytes / 32; // start of Order struct in slot array
    // Need os+0 through os+13 (externalCall is the last field at os+13).
    // Previously guarded only os+9, which allowed orders with non-empty externalCall
    // to pass with dln_external_call=None, defeating the iter-51 skip guard.
    if os + 13 >= slots.len() { return None; }

    // slot[1] = orderId bytes32
    let order_id_hex = format!("0x{}", hex::encode(&slots[1]));

    // Helpers
    let u64_at = |s: &[u8; 32]| u64::from_be_bytes(s[24..32].try_into().unwrap_or([0u8; 8]));
    let u128_str = |s: &[u8; 32]| {
        u128::from_be_bytes(s[16..32].try_into().unwrap_or([0u8; 16])).to_string()
    };

    // Read bytes field: offset is RELATIVE to order struct start, then length+data (multi-slot).
    let read_bytes_relative = |offset_slot: &[u8; 32]| -> Option<Vec<u8>> {
        let rel = u64_at(offset_slot) as usize / 32;
        let abs = os + rel;
        let len = u64_at(slots.get(abs)?) as usize;
        if len == 0 { return Some(vec![]); }
        let num_slots = len.div_ceil(32);
        let mut out = Vec::with_capacity(len);
        for i in 0..num_slots {
            let s = slots.get(abs + 1 + i)?;
            let take = if i + 1 == num_slots { len - i * 32 } else { 32 };
            out.extend_from_slice(&s[..take]);
        }
        Some(out)
    };

    let maker_nonce = u64_at(&slots[os]);
    // offsets: [os+1]=makerSrc, [os+2]=giveChainId, [os+3]=giveToken, [os+4]=giveAmount
    //          [os+5]=takeChainId, [os+6]=takeToken, [os+7]=takeAmount, [os+8]=receiverDst
    //          [os+9]=givePatchAuthoritySrc, [os+10]=orderAuthorityAddressDst,
    //          [os+11]=allowedTakerDst, [os+12]=allowedCancelBeneficiarySrc, [os+13]=externalCall
    let give_chain_id = u64_at(&slots[os + 2]);
    let give_amount   = u128_str(&slots[os + 4]);
    let take_chain_id = u64_at(&slots[os + 5]);
    let take_amount   = u128_str(&slots[os + 7]);

    let bytes_to_hex = |b: Vec<u8>| -> Option<String> {
        if b.is_empty() { None } else { Some(format!("0x{}", hex::encode(&b))) }
    };

    let maker_src   = read_bytes_relative(&slots[os + 1])
        .map(|b| format!("0x{}", hex::encode(&b)))
        .unwrap_or_else(|| "0x0000000000000000000000000000000000000000".into());
    let give_token  = read_bytes_relative(&slots[os + 3])
        .map(|b| format!("0x{}", hex::encode(&b)))
        .unwrap_or_else(|| "0x0000000000000000000000000000000000000000".into());
    let take_token  = read_bytes_relative(&slots[os + 6])
        .map(|b| format!("0x{}", hex::encode(&b)))
        .unwrap_or_else(|| "0x0000000000000000000000000000000000000000".into());
    let receiver    = read_bytes_relative(&slots[os + 8])
        .map(|b| format!("0x{}", hex::encode(&b)))
        .unwrap_or_else(|| "0x0000000000000000000000000000000000000000".into());

    // Optional authority/allowlist fields — must be decoded to reconstruct the correct orderId hash.
    let give_patch_authority_src = slots.get(os + 9)
        .and_then(|s| read_bytes_relative(s))
        .and_then(bytes_to_hex);
    let order_authority_address_dst = slots.get(os + 10)
        .and_then(|s| read_bytes_relative(s))
        .and_then(bytes_to_hex);
    let allowed_taker_dst = slots.get(os + 11)
        .and_then(|s| read_bytes_relative(s))
        .and_then(bytes_to_hex);
    let allowed_cancel_beneficiary_src = slots.get(os + 12)
        .and_then(|s| read_bytes_relative(s))
        .and_then(bytes_to_hex);
    let external_call = slots.get(os + 13)
        .and_then(|s| read_bytes_relative(s))
        .and_then(bytes_to_hex);

    let tx_hash = log["transactionHash"].as_str().unwrap_or("0x").to_string();

    // use_src_chain_id as give_chain_id verification: if giveChainId from log != src_chain_id
    // the order was created on a different chain than we polled — skip or trust the log value.
    let actual_src_chain = if give_chain_id > 0 { give_chain_id } else { src_chain_id };

    Some(Intent {
        id: format!("debridge_dln:{}", order_id_hex),
        protocol: "debridge_dln".to_string(),
        src_chain: actual_src_chain,
        dst_chain: take_chain_id,
        src_token: give_token.clone(),
        dst_token: take_token.clone(),
        amount: give_amount.clone(),
        depositor: maker_src,
        recipient: receiver,
        tx_hash,
        give_amount: Some(give_amount),
        take_amount: Some(take_amount.clone()),
        // output_amount = take_amount in wei so the executor can attach msg.value
        // for native-ETH fills (fulfillOrder requires msg.value == fulfillAmount when takeToken == 0x0).
        output_amount: Some(take_amount),
        order_id: Some(order_id_hex),
        maker_order_nonce: Some(maker_nonce),
        dln_give_patch_authority_src: give_patch_authority_src,
        dln_order_authority_address_dst: order_authority_address_dst,
        dln_allowed_taker_dst: allowed_taker_dst,
        dln_allowed_cancel_beneficiary_src: allowed_cancel_beneficiary_src,
        dln_external_call: external_call,
        detected_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        ..Default::default()
    })
}

// ── deBridge WS order feed ────────────────────────────────────────────────────

/// Real-time deBridge DLN order feed via the deBridge-managed WebSocket service.
///
/// Connects to `wss://dln-ws.debridge.finance/ws` with a Bearer token.
/// On connect, subscribes to new orders and requests all existing open orders.
/// Messages arrive in real-time (sub-second) vs the 12s eth_getLogs poll cycle.
///
/// Protocol (from dln-taker reference):
///   → send `{"Subscription":{"finalization_filter":{"confirmations_count":{}}}}`
///   → send `{"GetOrders":{"Created":{}}}`
///   ← receive `{"Order":{"subscription_id":"...","order_info":{...}}}`
///     where `order_info.order_info_status` contains `{"Created":{...}}` for new orders
///
/// The default `WS_API_KEY` is the publicly documented rate-limited key.
/// Set `DEBRIDGE_WS_API_KEY` env to override with a dedicated key.
pub struct DeBridgeWsPoller {
    pub ws_url: String,
    pub api_key: String,
}

impl DeBridgeWsPoller {
    pub fn default_mainnet() -> Self {
        let api_key = std::env::var("DEBRIDGE_WS_API_KEY")
            .unwrap_or_else(|_| "f8bb970668ba4cd15ee64bcbd24479bdf66c6bef9cbb9ece9f2ca3755bc2fe53".into());
        Self {
            ws_url: "wss://dln-ws.debridge.finance/ws".into(),
            api_key,
        }
    }

    pub async fn run(self, intent_tx: tokio::sync::mpsc::Sender<Intent>) {
        use tokio_tungstenite::{connect_async_tls_with_config, tungstenite::protocol::Message};
        use tokio_tungstenite::tungstenite::client::IntoClientRequest;
        use futures::{SinkExt, StreamExt};

        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

        loop {
            let mut req = match self.ws_url.as_str().into_client_request() {
                Ok(r) => r,
                Err(e) => {
                    warn!("deBridge WS: bad URL {}: {}", self.ws_url, e);
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    continue;
                }
            };
            let auth_val = match format!("Bearer {}", self.api_key)
                .parse::<reqwest::header::HeaderValue>()
            {
                Ok(v) => v,
                Err(e) => {
                    warn!("deBridge WS: invalid API key (non-ASCII?): {}", e);
                    tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                    continue;
                }
            };
            req.headers_mut().insert("Authorization", auth_val);

            let (ws_stream, _) = match connect_async_tls_with_config(req, None, false, None).await {
                Ok(s) => s,
                Err(e) => {
                    // 403 = invalid/missing API key; back off 60s to avoid log spam.
                    let is_auth = e.to_string().contains("403");
                    let delay = if is_auth { 60 } else { 5 };
                    if is_auth {
                        warn!("deBridge WS auth error (403) — set DEBRIDGE_WS_API_KEY. Retrying in {}s", delay);
                    } else {
                        warn!("deBridge WS connect error: {}. Retrying in {}s", e, delay);
                    }
                    tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
                    continue;
                }
            };

            info!("🔌 deBridge WS connected to {}", self.ws_url);
            let (mut write, mut read) = ws_stream.split();

            // Subscribe: no confirmation thresholds — accept all finalization states.
            let sub_msg = serde_json::json!({
                "Subscription": { "finalization_filter": { "confirmations_count": {} } }
            });
            let get_msg = serde_json::json!({ "GetOrders": { "Created": {} } });
            let _ = write.send(Message::Text(sub_msg.to_string().into())).await;
            let _ = write.send(Message::Text(get_msg.to_string().into())).await;

            // 90-second idle timeout: if no frame arrives (data, ping, or close),
            // treat the connection as stalled and reconnect. Without this, a TCP-level
            // hang (e.g. middlebox silently drops the connection) blocks forever.
            let idle_timeout = std::time::Duration::from_secs(90);
            loop {
                match tokio::time::timeout(idle_timeout, read.next()).await {
                    Err(_elapsed) => {
                        warn!("deBridge WS idle for {}s — reconnecting", idle_timeout.as_secs());
                        break;
                    }
                    Ok(None) => {
                        warn!("deBridge WS stream ended, reconnecting...");
                        break;
                    }
                    Ok(Some(msg)) => match msg {
                        Ok(Message::Text(text)) => {
                            if let Some(intent) = parse_debridge_ws_message(&text, &mut seen) {
                                info!("⚡ deBridge WS orderId={} {}→{} give={}",
                                    intent.order_id.as_deref().unwrap_or("?"),
                                    intent.src_chain, intent.dst_chain, intent.amount);
                                if intent_tx.send(intent).await.is_err() { return; }
                            }
                        }
                        Ok(Message::Ping(data)) => {
                            let _ = write.send(Message::Pong(data)).await;
                        }
                        Ok(Message::Close(_)) => {
                            warn!("deBridge WS closed, reconnecting...");
                            break;
                        }
                        Err(e) => {
                            warn!("deBridge WS error: {}, reconnecting...", e);
                            break;
                        }
                        _ => {}
                    },
                }
            }

            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }
    }
}

/// Parse a deBridge WS message and emit an Intent if it's a new `Created` order.
/// Returns None for Fulfilled/Cancelled/other status messages.
fn parse_debridge_ws_message(
    text: &str,
    seen: &mut std::collections::HashSet<String>,
) -> Option<Intent> {
    let v: serde_json::Value = serde_json::from_str(text).ok()?;
    let order_event = v.get("Order")?;
    let order_info = order_event.get("order_info")?;

    // Only process Created and ArchivalCreated statuses.
    let status = order_info.get("order_info_status")?;
    let is_created = status.get("Created").is_some() || status.get("ArchivalCreated").is_some();
    if !is_created { return None; }

    let order_id_hex = order_info.get("order_id")?.as_str()?;
    // order_id is a 32-byte hex string WITHOUT 0x prefix from the WS
    let order_id = if order_id_hex.starts_with("0x") {
        order_id_hex.to_string()
    } else {
        format!("0x{}", order_id_hex)
    };

    // Dedup: evict (clear) when cap is hit — a brief replay window is acceptable
    // vs unbounded memory growth. Cap at 50 000 order IDs (~3.3 MB).
    if seen.len() >= 50_000 { seen.clear(); }
    if !seen.insert(order_id.clone()) { return None; }

    let order = order_info.get("order")?;

    // Decode give (source) and take (dst) offers.
    // Chain IDs and amounts are hex-encoded big-endian 32-byte strings.
    let hex_to_u64 = |s: &str| -> u64 {
        let clean = s.trim_start_matches("0x");
        u64::from_str_radix(clean.trim_start_matches('0').get(..16).unwrap_or(clean), 16).unwrap_or(0)
    };
    let hex_to_u128 = |s: &str| -> u128 {
        let clean = s.trim_start_matches("0x");
        u128::from_str_radix(clean.trim_start_matches('0').get(..32).unwrap_or(clean), 16).unwrap_or(0)
    };
    let hex_to_addr = |s: &str| -> String {
        // token/address fields are 32-byte hex; last 20 bytes = address
        let clean = s.trim_start_matches("0x");
        if clean.len() >= 40 {
            format!("0x{}", &clean[clean.len()-40..])
        } else {
            format!("0x{}", clean)
        }
    };

    let give = order.get("give")?;
    let take = order.get("take")?;

    let src_chain = hex_to_u64(give.get("chain_id")?.as_str()?);
    let dst_chain = hex_to_u64(take.get("chain_id")?.as_str()?);
    let give_amount = hex_to_u128(give.get("amount")?.as_str()?).to_string();
    let take_amount = hex_to_u128(take.get("amount")?.as_str()?).to_string();
    let src_token = hex_to_addr(give.get("token_address")?.as_str()?);
    let dst_token = hex_to_addr(take.get("token_address")?.as_str()?);

    // Skip non-EVM destination chains — same filter as DeBridgePoller logs path.
    // Exception: Solana (100_000_001) is handled by DlnSolanaFiller.
    let is_solana_dst = if let Some(name) = debridge_non_evm_chain_name(dst_chain) {
        if dst_chain != 100_000_001 {
            tracing::debug!("deBridge WS skip non-EVM dst chain={} ({})", dst_chain, name);
            return None;
        }
        // Solana destination — allow through for DLN Solana fill path.
        true
    } else {
        false
    };
    // Skip exotic tokens (same filter as DeBridgePoller).
    // Exception: for Solana destinations the dst_token is a base58 SPL mint, not an EVM hex.
    if !is_solana_dst && !is_supported_fill_token(&dst_token) {
        tracing::debug!("deBridge WS skip exotic dst_token={}", dst_token);
        return None;
    }

    let maker_src = order.get("maker_src")?.as_str()?;
    let receiver = order.get("receiver_dst")?.as_str()?;
    let maker_nonce: u64 = order.get("maker_order_nonce")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    let allowed_taker_raw = order.get("allowed_taker_dst").and_then(|v| v.as_str());
    let allowed_taker = allowed_taker_raw.filter(|s| !s.is_empty() && *s != "0x").map(|s| {
        if s.starts_with("0x") { s.to_string() } else { format!("0x{}", s) }
    });

    let give_patch = order.get("give_patch_authority_src").and_then(|v| v.as_str())
        .filter(|s| !s.is_empty()).map(|s| format!("0x{}", s.trim_start_matches("0x")));
    let order_auth_dst = order.get("order_authority_address_dst").and_then(|v| v.as_str())
        .filter(|s| !s.is_empty()).map(|s| format!("0x{}", s.trim_start_matches("0x")));
    let cancel_ben = order.get("allowed_cancel_beneficiary_src").and_then(|v| v.as_str())
        .filter(|s| !s.is_empty() && *s != "null").map(|s| format!("0x{}", s.trim_start_matches("0x")));
    // external_call: must be populated so the iter-51 skip guard in lambda_controller fires.
    // If this field is absent from the WS message we treat it as empty (None = no calldata).
    let external_call = order.get("external_call").and_then(|v| v.as_str())
        .filter(|s| {
            let clean = s.trim_start_matches("0x");
            !clean.is_empty() && clean.chars().any(|c| c != '0')
        })
        .map(|s| format!("0x{}", s.trim_start_matches("0x")));

    // Get tx_hash from finalization_info if present
    let tx_hash = status.get("Created")
        .and_then(|c| c.get("finalization_info"))
        .and_then(|fi| {
            fi.get("Finalized").or_else(|| fi.get("Confirmed"))
        })
        .and_then(|f| f.get("transaction_hash"))
        .and_then(|h| h.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| order_id.clone());

    let maker_src_addr = {
        let clean = maker_src.trim_start_matches("0x").to_lowercase();
        if clean.len() >= 40 {
            format!("0x{}", &clean[clean.len()-40..])
        } else {
            format!("0x{}", clean)
        }
    };
    let receiver_addr = {
        let clean = receiver.trim_start_matches("0x").to_lowercase();
        if clean.len() >= 40 {
            format!("0x{}", &clean[clean.len()-40..])
        } else {
            format!("0x{}", clean)
        }
    };

    Some(Intent {
        id: format!("debridge_dln:{}", order_id),
        protocol: "debridge_dln".to_string(),
        src_chain,
        dst_chain,
        src_token,
        dst_token,
        amount: give_amount.clone(),
        depositor: maker_src_addr,
        recipient: receiver_addr,
        tx_hash,
        give_amount: Some(give_amount),
        take_amount: Some(take_amount),
        order_id: Some(order_id),
        maker_order_nonce: Some(maker_nonce),
        dln_give_patch_authority_src: give_patch,
        dln_order_authority_address_dst: order_auth_dst,
        dln_allowed_taker_dst: allowed_taker,
        dln_allowed_cancel_beneficiary_src: cancel_ben,
        dln_external_call: external_call,
        is_solana_destination: if is_solana_dst { Some(true) } else { None },
        detected_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        ..Default::default()
    })
}

// ── Mayan Swift order params decoder ─────────────────────────────────────────

/// Decoded fields from a Mayan Swift V2 `createOrderWithToken` or
/// `createOrderWithEth` calldata on the source chain transaction.
#[derive(Debug, Default)]
pub struct MayanOrderParams {
    /// 32-byte random salt (hex, no 0x prefix).
    pub random: Option<String>,
    pub cancel_fee: Option<u64>,
    pub refund_fee: Option<u64>,
    pub gas_drop: Option<u64>,
    pub deadline: Option<u64>,
    pub referrer_addr: Option<String>,
    pub referrer_bps: Option<u8>,
    pub auction_mode: Option<u8>,
}

/// Fetch `eth_getTransactionByHash` on `src_chain` for `tx_hash` and extract
/// the Mayan Swift V2 OrderParams fields from the `createOrderWithToken` calldata.
///
/// The Mayan Forwarder wraps an inner `createOrderWithToken` call. We locate the
/// inner selector `0xe4269fc4` (MayanSwiftV2.createOrderWithToken) inside the
/// calldata and decode the Order struct at that position.
///
/// Order struct layout (all slots 32 bytes each, after the 4-byte selector):
///   slot 0: tokenIn (address, right-padded)
///   slot 1: amountIn (uint256)
///   slot 2: payloadType (uint8)
///   slot 3: trader (bytes32)
///   slot 4: tokenOut (bytes32)
///   slot 5: destChainId (uint16)
///   slot 6: destAddr (bytes32)
///   slot 7: minAmountOut (uint64)
///   slot 8: gasDrop (uint64)
///   slot 9: cancelFee (uint64)
///  slot 10: refundFee (uint64)
///  slot 11: deadline (uint64)
///  slot 12: referrerAddr (bytes32)
///  slot 13: referrerBps (uint8)
///  slot 14: auctionMode (uint8)
///  slot 15: random (bytes32)
pub async fn fetch_mayan_order_params(
    http: &reqwest::Client,
    src_chain: u64,
    tx_hash: &str,
) -> MayanOrderParams {
    if tx_hash.is_empty() || tx_hash == "0x" {
        return MayanOrderParams::default();
    }
    let rpc = match src_chain {
        1     => "https://eth.llamarpc.com",
        10    => "https://mainnet.optimism.io",
        56    => "https://bsc-dataseed.binance.org",
        137   => "https://polygon-bor-rpc.publicnode.com",
        8453  => "https://mainnet.base.org",
        42161 => "https://arb1.arbitrum.io/rpc",
        43114 => "https://api.avax.network/ext/bc/C/rpc",
        _     => return MayanOrderParams::default(),
    };

    let body = serde_json::json!({
        "jsonrpc": "2.0", "id": 1,
        "method": "eth_getTransactionByHash",
        "params": [tx_hash]
    });
    let resp = match http.post(rpc).json(&body).send().await {
        Ok(r) => r,
        Err(_) => return MayanOrderParams::default(),
    };
    let v: serde_json::Value = match resp.json().await {
        Ok(v) => v,
        Err(_) => return MayanOrderParams::default(),
    };
    let input = match v["result"]["input"].as_str() {
        Some(s) if s.len() > 10 => s.trim_start_matches("0x").to_string(),
        _ => return MayanOrderParams::default(),
    };

    // Find the inner createOrderWithToken selector (0xe4269fc4) inside the calldata.
    // It appears as the first 4 bytes of the inner calldata argument passed to
    // forwardERC20 / forwardEth by the Mayan Forwarder contract.
    const CREATE_ORDER_SEL: &str = "e4269fc4";
    let pos = match input.find(CREATE_ORDER_SEL) {
        Some(p) if p % 2 == 0 => p,
        _ => return MayanOrderParams::default(),
    };

    // Skip past selector (8 hex chars = 4 bytes). Then read the ABI-encoded Order struct.
    // The first 3 slots (tokenIn, amountIn, ... are skipped in our interest) but we need
    // to be careful: the outer function may have ABI-encoded the struct itself, so there may
    // be an offset pointer before the struct data. We detect this by checking if slot 0
    // looks like an offset (< 256) vs an address (> 2^32).
    let data = &input[pos + 8..]; // skip the selector

    // Parse as 32-byte (64 hex char) slots
    let slots: Vec<&str> = (0..data.len() / 64)
        .map(|i| &data[i * 64..(i + 1) * 64])
        .collect();

    // Slot layout after selector (0-indexed, no offset pointer — slot 0 is tokenIn address,
    // which is always > 4096 so offset_shift stays 0):
    // 0=tokenIn, 1=amountIn, then Order struct fields inline:
    // 2=payloadType, 3=trader, 4=destAddr, 5=destChainId, 6=referrerAddr,
    // 7=tokenOut, 8=minAmountOut, 9=gasDrop, 10=cancelFee, 11=refundFee,
    // 12=deadline, 13=referrerBps, 14=auctionMode, 15=random
    // (matches MayanSwiftCreate::Order field order in rebalancer.rs / on-chain ABI)
    let offset_shift: usize = if slots.first().map_or(false, |s| {
        u64::from_str_radix(s, 16).unwrap_or(u64::MAX) < 4096
    }) { 1 } else { 0 };

    let slot = |i: usize| slots.get(i + offset_shift).copied().unwrap_or("");
    let parse_u64 = |s: &str| u64::from_str_radix(s.trim_start_matches('0'), 16).ok().filter(|&v| v > 0);

    MayanOrderParams {
        random: {
            let r = slot(15);
            if r.len() == 64 && r != "0".repeat(64) { Some(r.to_string()) } else { None }
        },
        gas_drop: parse_u64(slot(9)),
        cancel_fee: parse_u64(slot(10)),
        refund_fee: parse_u64(slot(11)),
        deadline: parse_u64(slot(12)),
        referrer_addr: {
            let r = slot(6);
            if r.len() == 64 && r != "0".repeat(64) { Some(format!("0x{}", r)) } else { None }
        },
        referrer_bps: u8::from_str_radix(slot(13).trim_start_matches('0'), 16).ok(),
        auction_mode: u8::from_str_radix(slot(14).trim_start_matches('0'), 16).ok(),
    }
}

// ── Mayan Swift poller ────────────────────────────────────────────────────────

/// Mayan Swift ORDER_CREATED poller.
///
/// Polls `explorer-api.mayan.finance/v3/swaps` for SWIFT_V2 `ORDER_CREATED`
/// entries whose destination chain is an EVM network we can fill. Mayan uses
/// its own chain-ID namespace: 2=Eth, 4=BSC, 5=Polygon, 6=Avalanche, 23=Base,
/// 30=Arbitrum, 47=Optimism. We map those to standard EVM chain IDs.
///
/// Orders settle in 7-17 seconds, so we poll at 3s and deduplicate by orderHash.
pub struct MayanPoller {
    /// Poll interval in seconds (default: 3).
    pub poll_interval_secs: u64,
    /// Max orders per poll.
    pub limit: usize,
}

/// Mayan internal chain ID → EVM chain ID.
/// Mayan uses Wormhole chain IDs: ETH=2, BSC=4, Polygon=5, Avalanche=6,
/// Arbitrum=23, Optimism=24, Base=30.
fn mayan_chain_to_evm(mayan: &str) -> Option<u64> {
    match mayan {
        "2"  => Some(1),      // Ethereum
        "4"  => Some(56),     // BSC
        "5"  => Some(137),    // Polygon
        "6"  => Some(43114),  // Avalanche
        "23" => Some(42161),  // Arbitrum
        "24" => Some(10),     // Optimism
        "30" => Some(8453),   // Base
        _    => None,
    }
}

impl Default for MayanPoller {
    fn default() -> Self {
        Self { poll_interval_secs: 3, limit: 20 }
    }
}

impl MayanPoller {
    /// Run forever, emitting fillable Mayan Swift EVM intents to `intent_tx`.
    pub async fn run(self, intent_tx: tokio::sync::mpsc::Sender<Intent>) {
        use std::collections::HashSet;
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .user_agent("Mozilla/5.0 (compatible; taifoon-solver/1.0)")
            .build()
            .unwrap_or_default();
        let mut seen: HashSet<String> = HashSet::new();

        loop {
            let url = format!(
                "https://explorer-api.mayan.finance/v3/swaps?limit={}&service=SWIFT_V2",
                self.limit
            );
            let resp = match client.get(&url).send().await {
                Ok(r) => r,
                Err(e) => {
                    tracing::debug!("MayanPoller request error: {}", e);
                    tokio::time::sleep(std::time::Duration::from_secs(self.poll_interval_secs)).await;
                    continue;
                }
            };
            if !resp.status().is_success() {
                tracing::warn!("MayanPoller HTTP {}: {}", resp.status(), url);
                tokio::time::sleep(std::time::Duration::from_secs(self.poll_interval_secs)).await;
                continue;
            }
            let body: serde_json::Value = match resp.json().await {
                Ok(b) => b,
                Err(e) => {
                    tracing::debug!("MayanPoller parse error: {}", e);
                    tokio::time::sleep(std::time::Duration::from_secs(self.poll_interval_secs)).await;
                    continue;
                }
            };
            let orders = match body.get("data").and_then(|v| v.as_array()) {
                Some(arr) => arr.clone(),
                None => {
                    tokio::time::sleep(std::time::Duration::from_secs(self.poll_interval_secs)).await;
                    continue;
                }
            };

            for order in &orders {
                let st = order.get("status").and_then(|v| v.as_str()).unwrap_or("");
                if st != "ORDER_CREATED" {
                    continue;
                }
                let order_hash = match order.get("orderHash").and_then(|v| v.as_str()) {
                    Some(h) if !h.is_empty() => h.to_string(),
                    _ => continue,
                };
                // Evict oldest half when the seen set reaches 20 000 entries to
                // prevent unbounded growth in long-running processes.
                if seen.len() >= 20_000 {
                    let mut keys: Vec<String> = seen.iter().cloned().collect();
                    keys.sort_unstable();
                    for k in keys.into_iter().take(10_000) {
                        seen.remove(&k);
                    }
                }
                if !seen.insert(order_hash.clone()) {
                    continue;
                }

                let dst_chain_mayan = order.get("destChain").and_then(|v| v.as_str()).unwrap_or("0");
                // destChain="1" means Solana destination — use sentinel chain id.
                // All other non-EVM dest chains are skipped.
                let is_solana_dest = dst_chain_mayan == "1";
                let dst_chain = if is_solana_dest {
                    1_399_811_149u64
                } else {
                    match mayan_chain_to_evm(dst_chain_mayan) {
                        Some(c) => c,
                        None => {
                            tracing::debug!("MayanPoller skip non-EVM dest chain {}", dst_chain_mayan);
                            continue;
                        }
                    }
                };

                let src_chain_mayan = order.get("sourceChain").and_then(|v| v.as_str()).unwrap_or("0");
                let _swap_chain = order.get("swapChain").and_then(|v| v.as_str()).unwrap_or("0");
                let src_chain = mayan_chain_to_evm(src_chain_mayan).unwrap_or(0);

                let from_token = order.get("fromTokenAddress").and_then(|v| v.as_str()).unwrap_or("0x0").to_string();
                let to_token = order.get("toTokenAddress").and_then(|v| v.as_str()).unwrap_or("0x0").to_string();
                let dest_addr = order.get("destAddress").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let trader = order.get("trader").and_then(|v| v.as_str()).unwrap_or("0x0").to_string();

                // For EVM-destination orders: skip if any address is non-EVM (base58 Solana mint).
                // For Solana-destination orders: dest_addr is a Solana pubkey (base58) — allow it.
                if !is_solana_dest {
                    let is_evm_addr = |s: &str| s.starts_with("0x") || s.starts_with("0X");
                    if !is_evm_addr(&from_token) || !is_evm_addr(&to_token) || !is_evm_addr(&dest_addr) {
                        tracing::debug!("MayanPoller skip non-EVM addresses on order {}", &order_hash[..20.min(order_hash.len())]);
                        continue;
                    }
                }

                let source_tx = order.get("sourceTxHash").and_then(|v| v.as_str()).unwrap_or("0x").to_string();
                let state_addr = order.get("stateAddr").and_then(|v| v.as_str()).map(|s| s.to_string());

                let from_amount_str = order.get("fromAmount").and_then(|v| v.as_str()).unwrap_or("0");
                let to_amount_str = order.get("toAmount").and_then(|v| v.as_str());

                let auction_mode_api = order.get("auctionMode").and_then(|v| v.as_u64()).unwrap_or(2) as u8;
                // is_solana_source flag: true only when destination is Solana (fill happens on Solana).
                // Solana-source→EVM orders (swap_chain==1, src_chain==0) are filled on EVM,
                // so they must NOT trigger the Solana broadcast path.
                let is_solana_src = is_solana_dest;

                // Decode OrderParams from source tx calldata for EVM-source orders.
                let order_params = if !is_solana_src && src_chain != 0 && !source_tx.is_empty() && source_tx != "0x" {
                    fetch_mayan_order_params(&client, src_chain, &source_tx).await
                } else {
                    MayanOrderParams::default()
                };

                let effective_auction_mode = order_params.auction_mode.unwrap_or(auction_mode_api);

                info!("📡 MayanPoller ORDER_CREATED orderHash={} src={}({}) dst={}({}) {}→{} mode={} sol_fill={} random={}",
                    &order_hash[..20.min(order_hash.len())],
                    src_chain_mayan, src_chain,
                    dst_chain_mayan, dst_chain,
                    order.get("fromTokenSymbol").and_then(|v| v.as_str()).unwrap_or("?"),
                    order.get("toTokenSymbol").and_then(|v| v.as_str()).unwrap_or("?"),
                    effective_auction_mode,
                    is_solana_src,
                    if order_params.random.is_some() { "✅" } else { "missing" });

                let intent = Intent {
                    id: format!("mayan_swift:{}", order_hash),
                    protocol: "mayan_swift".to_string(),
                    src_chain,
                    dst_chain,
                    src_token: from_token,
                    dst_token: to_token,
                    amount: from_amount_str.to_string(),
                    depositor: trader.clone(),
                    recipient: dest_addr,
                    tx_hash: source_tx,
                    output_amount: to_amount_str.map(|s| s.to_string()),
                    mayan_order_id: Some(order_hash),
                    trader: Some(trader),
                    is_solana_source: Some(is_solana_src),
                    // stateAddr is the Solana order PDA — required for Mayan Solana fulfill.
                    state_account: state_addr,
                    batch_id: None,
                    detected_at: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs(),
                    mayan_random: order_params.random,
                    mayan_auction_mode: Some(effective_auction_mode),
                    mayan_cancel_fee: order_params.cancel_fee,
                    mayan_refund_fee: order_params.refund_fee,
                    mayan_referrer_addr: order_params.referrer_addr,
                    mayan_referrer_bps: order_params.referrer_bps,
                    mayan_gas_drop: order_params.gas_drop,
                    deadline: order_params.deadline,
                    ..Default::default()
                };

                if intent_tx.send(intent).await.is_err() {
                    return;
                }
            }

            tokio::time::sleep(std::time::Duration::from_secs(self.poll_interval_secs)).await;
        }
    }
}

// ── GenomeClient ──────────────────────────────────────────────────────────────

impl GenomeClient {
    /// Create new genome client
    pub fn new(sse_url: impl Into<String>) -> Self {
        Self {
            sse_url: sse_url.into(),
            client: reqwest::Client::new(),
        }
    }

    /// Subscribe to genome stream AND spawn protocol pollers in parallel.
    ///
    /// Use this instead of `subscribe` when the genome SSE stream does not emit
    /// `entity: "proto"` deposit events. The Across poller runs alongside the SSE
    /// consumer and feeds intents from the Across REST API into the same channel.
    pub async fn subscribe_with_pollers(
        &self,
        intent_tx: mpsc::Sender<Intent>,
        pollers: Vec<AcrossPoller>,
    ) -> Result<()> {
        for poller in pollers {
            let tx = intent_tx.clone();
            tokio::spawn(async move { poller.run(tx).await });
        }
        self.subscribe(intent_tx).await
    }

    /// Like `subscribe_with_pollers` but also spawns deBridge and Mayan pollers.
    pub async fn subscribe_with_all_pollers(
        &self,
        intent_tx: mpsc::Sender<Intent>,
        across_pollers: Vec<AcrossPoller>,
        debridge_poller: Option<DeBridgePoller>,
    ) -> Result<()> {
        self.subscribe_with_all_pollers_and_mayan(
            intent_tx, across_pollers, debridge_poller, Some(MayanPoller::default()), Some(DeBridgeWsPoller::default_mainnet())
        ).await
    }

    /// Full poller suite: Across + deBridge eth_getLogs + deBridge WS + Mayan.
    pub async fn subscribe_with_all_pollers_and_mayan(
        &self,
        intent_tx: mpsc::Sender<Intent>,
        across_pollers: Vec<AcrossPoller>,
        debridge_poller: Option<DeBridgePoller>,
        mayan_poller: Option<MayanPoller>,
        debridge_ws: Option<DeBridgeWsPoller>,
    ) -> Result<()> {
        for poller in across_pollers {
            let tx = intent_tx.clone();
            tokio::spawn(async move { poller.run(tx).await });
        }
        if let Some(dp) = debridge_poller {
            let tx = intent_tx.clone();
            tokio::spawn(async move { dp.run(tx).await });
        }
        if let Some(ws) = debridge_ws {
            let tx = intent_tx.clone();
            tokio::spawn(async move { ws.run(tx).await });
        }
        if let Some(mp) = mayan_poller {
            let tx = intent_tx.clone();
            tokio::spawn(async move { mp.run(tx).await });
        }
        self.subscribe(intent_tx).await
    }

    /// Subscribe to genome stream and send intents to channel
    pub async fn subscribe(&self, intent_tx: mpsc::Sender<Intent>) -> Result<()> {
        info!("🔌 Connecting to genome stream: {}", self.sse_url);

        let mut backoff_secs: u64 = 2;
        loop {
            match self.subscribe_internal(&intent_tx).await {
                Ok(_) => {
                    warn!("Genome stream ended unexpectedly, reconnecting in {}s...", backoff_secs);
                    // Clean close — stream was healthy; reset backoff.
                    backoff_secs = 2;
                }
                Err(e) => {
                    error!("Genome stream error: {}, reconnecting in {}s...", e, backoff_secs);
                    // Exponential backoff capped at 60s for persistent failures.
                    backoff_secs = (backoff_secs * 2).min(60);
                }
            }

            tokio::time::sleep(std::time::Duration::from_secs(backoff_secs)).await;
        }
    }

    async fn subscribe_internal(&self, intent_tx: &mpsc::Sender<Intent>) -> Result<()> {
        let mut response = self
            .client
            .get(&self.sse_url)
            .send()
            .await
            .context("Failed to connect to genome stream")?;

        info!("✅ Connected to genome stream");

        let mut buffer = String::new();
        // Guard against unbounded growth from malformed/truncated SSE data.
        const MAX_BUFFER_BYTES: usize = 512 * 1024; // 512 KiB

        while let Some(chunk) = response.chunk().await? {
            let text = String::from_utf8_lossy(&chunk);
            buffer.push_str(&text);

            // Drop the buffer if it exceeds the cap without a complete event —
            // indicates a malformed stream or a very large non-intent event.
            if buffer.len() > MAX_BUFFER_BYTES && !buffer.contains("\n\n") {
                tracing::warn!("SSE buffer overflow ({} bytes, no complete event) — discarding", buffer.len());
                buffer.clear();
                continue;
            }

            // Process complete SSE events
            while let Some(event_end) = buffer.find("\n\n") {
                let event_text = buffer[..event_end].to_string();
                buffer = buffer[event_end + 2..].to_string();

                if let Some(intent) = self.parse_sse_event(&event_text) {
                    info!("🎯 New intent detected: {} ({} → {})", intent.protocol, intent.src_chain, intent.dst_chain);

                    if intent_tx.send(intent).await.is_err() {
                        warn!("Intent receiver dropped, stopping stream");
                        return Ok(());
                    }
                }
            }
        }

        Ok(())
    }

    fn parse_sse_event(&self, event_text: &str) -> Option<Intent> {
        // SSE format:
        // event: genome
        // data: {...}
        //
        // Per SSE spec, multiple `data:` lines are concatenated with \n.
        // In practice genome emits single-line JSON but we handle multi-line
        // correctly to avoid silent data truncation.

        let mut event_type = None;
        let mut data_parts: Vec<&str> = Vec::new();

        for line in event_text.lines() {
            if let Some(content) = line.strip_prefix("event: ") {
                event_type = Some(content.trim());
            } else if let Some(content) = line.strip_prefix("data: ") {
                data_parts.push(content.trim());
            }
        }

        // Accept both "genome" and "genome_entry" event types
        if event_type != Some("genome") && event_type != Some("genome_entry") {
            return None;
        }

        if data_parts.is_empty() { return None; }
        let data_owned;
        let data: &str = if data_parts.len() == 1 {
            data_parts[0]
        } else {
            data_owned = data_parts.join("\n");
            &data_owned
        };

        // Parse JSON data, tolerating legacy/canonical key duplicates.
        let genome_event: GenomeEvent = match GenomeEvent::from_json_str(data) {
            Ok(event) => event,
            Err(e) => {
                warn!("Failed to parse genome event: {}", e);
                return None;
            }
        };

        // Accept both "proto" (old format) and "order" (new multi-protocol format)
        // Filter for deposit/placed/executed actions (cross-chain intent initiated)
        if genome_event.entity != "proto" && genome_event.entity != "order" {
            // Log every non-infra entity so we can discover deBridge/Mayan event shapes.
            let e = &genome_event.entity;
            if e != "block" && e != "gas" && e != "superroot" && e != "finality" {
                info!("🔎 UNKNOWN entity='{}' action='{}' protocol={:?} id={:?} addr={}",
                    genome_event.entity, genome_event.action,
                    genome_event.protocol, genome_event.id, genome_event.addr);
            }
            return None;
        }

        // Skip non-actionable states (only process new/pending orders)
        if genome_event.action != "deposit"
            && genome_event.action != "placed"
            && genome_event.action != "executed" {
            // Known terminal/lifecycle actions: debug only to avoid log spam.
            // Unknown actions stay at info so new protocol shapes are discoverable.
            let known = matches!(genome_event.action.as_str(),
                "expired" | "claimed" | "executed" | "fill" | "ingest" | "snapshot" | "cancelled" | "refunded");
            if known {
                tracing::debug!("genome skip action='{}' entity='{}' protocol={:?}",
                    genome_event.action, genome_event.entity, genome_event.protocol);
            } else {
                info!("🔎 UNKNOWN action='{}' entity='{}' protocol={:?} id={:?} addr={}",
                    genome_event.action, genome_event.entity,
                    genome_event.protocol, genome_event.id, genome_event.addr);
            }
            return None;
        }

        // Mayan Swift orders arrive via MayanPoller (REST API) with correct decimal
        // amounts. The SSE stream puts the 32-byte order hash in input_amount, which
        // parses as a ~$648T notional. Drop SSE Mayan events here; MayanPoller handles them.
        let proto_hint = genome_event.protocol.as_deref().unwrap_or("");
        if proto_hint.contains("mayan") || proto_hint.contains("swift") {
            return None;
        }

        // Log all matching protocol events at info level so we can see bridge/tool fields.
        info!("🔍 genome proto event: entity={} action={} protocol={:?} id={:?} bridge={:?} tool={:?} src={}→dst={} addr={}",
            genome_event.entity, genome_event.action,
            genome_event.protocol, genome_event.id,
            genome_event.bridge, genome_event.tool,
            genome_event.src_chain.unwrap_or(0), genome_event.dst_chain.unwrap_or(0),
            genome_event.addr);

        // Convert to Intent
        match Intent::from_genome_event(genome_event) {
            Ok(intent) => Some(intent),
            Err(e) => {
                warn!("Failed to convert genome event to intent: {}", e);
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_genome_event() {
        let event_json = r#"{
            "address": "T:1745678/proto:lifi_v2/deposit:1:0xabc123",
            "entity": "proto",
            "id": "lifi_v2",
            "action": "deposit",
            "chain_id": 1,
            "ref_hash": "0xabc123",
            "src_chain": 1,
            "dst_chain": 42161,
            "depositor": "0xuser123",
            "recipient": "0xuser123",
            "src_token": "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48",
            "input_amount": "1000000000",
            "ts": 1745678400
        }"#;

        let genome_event: GenomeEvent = serde_json::from_str(event_json).unwrap();
        let intent = Intent::from_genome_event(genome_event).unwrap();

        assert_eq!(intent.protocol, "lifi_v2");
        assert_eq!(intent.src_chain, 1);
        assert_eq!(intent.dst_chain, 42161);
        assert_eq!(intent.amount, "1000000000");
    }

    #[test]
    fn test_lifi_tx_hash_extracted_from_addr() {
        // Genome LiFi events omit ref_hash but embed the Diamond tx in the addr path segment.
        // Verify that tx_hash is correctly extracted so li.quest can be called.
        let event_json = r#"{
            "addr": "T:1745678/proto:lifi_v2/deposit:1:0x8f402c380754a14c8a216c67e219af96c8a449b6b6cd08553d455945e616bba4",
            "entity": "proto",
            "id": "lifi_v2",
            "action": "deposit",
            "chain_id": 59144,
            "src_chain": 59144,
            "dst_chain": 8453,
            "depositor": "0xuser123",
            "src_token": "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48",
            "input_amount": "2213850",
            "ts": 1745678400
        }"#;

        let genome_event: GenomeEvent = serde_json::from_str(event_json).unwrap();
        let intent = Intent::from_genome_event(genome_event).unwrap();

        assert_eq!(
            intent.tx_hash,
            "0x8f402c380754a14c8a216c67e219af96c8a449b6b6cd08553d455945e616bba4",
            "tx_hash must be extracted from addr for LiFi genome events without ref_hash"
        );
        assert!(!intent.tx_hash.starts_with("synthetic_"), "should not be synthetic");
    }

    #[test]
    fn test_decode_dln_order_created_log_round_trip() {
        // Synthetic ABI-encoded OrderCreated log verified against the real Arbitrum DLN layout:
        // slot[0]=offset_to_order(224), slot[1]=orderId, slots[2..6]=padding, slots[7..]=Order struct
        let mut slots: Vec<[u8; 32]> = Vec::new();
        let p = |v: u128| -> [u8; 32] { let mut s = [0u8; 32]; s[16..].copy_from_slice(&v.to_be_bytes()); s };
        let p64 = |v: u64| -> [u8; 32] { let mut s = [0u8; 32]; s[24..].copy_from_slice(&v.to_be_bytes()); s };
        let addr = |hex: &str| -> [u8; 32] { let b = hex::decode(hex).unwrap(); let mut s = [0u8; 32]; s[..b.len()].copy_from_slice(&b); s };

        // Top-level
        slots.push(p64(224));  // [0] order offset = 7*32
        slots.push(addr("4f5e6d7c8b9a0123456789abcdef0123456789abcdef0123456789abcdef0123")); // [1] orderId
        for _ in 0..5 { slots.push([0u8; 32]); }  // [2..6] padding

        // Order struct at slot[7] (os=7). Offsets are RELATIVE to Order struct start.
        // Dynamic data starts 9 slots (288 bytes) after Order struct start.
        slots.push(p64(12345));          // [os+0] makerOrderNonce
        slots.push(p64(9 * 32));         // [os+1] makerSrc offset = 288
        slots.push(p64(42161));          // [os+2] giveChainId
        slots.push(p64(9 * 32 + 64));    // [os+3] giveToken offset = 352
        slots.push(p(1_000_000_000));    // [os+4] giveAmount
        slots.push(p64(10));             // [os+5] takeChainId
        slots.push(p64(9 * 32 + 128));   // [os+6] takeToken offset = 416
        slots.push(p(998_000_000));      // [os+7] takeAmount
        slots.push(p64(9 * 32 + 192));   // [os+8] receiverDst offset = 480
        // Dynamic data: each = length(32) + data(padded to 32)
        let push_bytes = |slots: &mut Vec<[u8; 32]>, hex: &str| {
            let b = hex::decode(hex).unwrap();
            slots.push(p64(b.len() as u64));
            let mut d = [0u8; 32]; d[..b.len()].copy_from_slice(&b); slots.push(d);
        };
        push_bytes(&mut slots, "9a8b7c6d5e4f3a2b1c0d9e8f7a6b5c4d3e2f1a0b"); // makerSrc
        push_bytes(&mut slots, "a0b86991c6218b36c1d19d4a2e9eb0ce3606eb48"); // giveToken
        push_bytes(&mut slots, "0b2c639c533813f4aa9d7837caf62653d097ff85"); // takeToken
        push_bytes(&mut slots, "abcdef1234567890abcdef1234567890abcdef12"); // receiverDst

        let data_hex = format!("0x{}", slots.iter().map(|s| hex::encode(s)).collect::<String>());
        let log = serde_json::json!({
            "data": data_hex,
            "transactionHash": "0xdeadbeefdeadbeef"
        });

        let intent = decode_dln_order_created_log(&log, 42161).expect("should decode");
        assert_eq!(intent.maker_order_nonce, Some(12345));
        assert_eq!(intent.src_chain, 42161);
        assert_eq!(intent.dst_chain, 10);
        assert_eq!(intent.give_amount.as_deref(), Some("1000000000"));
        assert_eq!(intent.take_amount.as_deref(), Some("998000000"));
        assert_eq!(intent.depositor, "0x9a8b7c6d5e4f3a2b1c0d9e8f7a6b5c4d3e2f1a0b");
        assert_eq!(intent.src_token, "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48");
        assert_eq!(intent.dst_token, "0x0b2c639c533813f4aa9d7837caf62653d097ff85");
        assert_eq!(intent.recipient, "0xabcdef1234567890abcdef1234567890abcdef12");
        assert!(intent.order_id.as_deref().unwrap().starts_with("0x4f5e6d7c"));
    }
}
