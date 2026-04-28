use anyhow::{Context, Result};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
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
    #[serde(default)]
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

        // Use ref_hash as tx hash, with fallback to generated ID for protocols without tx_hash
        let tx_hash = event
            .reference.clone()
            .or_else(|| event.order_id.clone())
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
                    .unwrap()
                    .as_millis() as u64 / 1000
            }),
            output_amount: event.output_amount,
            deposit_id: event.deposit_id,
            maker_order_nonce: event.maker_order_nonce,
            give_amount: event.give_amount,
            take_amount: event.take_amount,
            order_id: event.order_id,
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
}

impl AcrossPoller {
    pub fn default_mainnet() -> Self {
        Self {
            dst_chains: vec![8453, 10, 42161, 1, 56],
            poll_interval_secs: 8,
            limit: 20,
        }
    }

    /// Run forever, sending fillable Across intents to `intent_tx`.
    /// Tracks seen depositIds in a local set to avoid re-emitting.
    pub async fn run(self, intent_tx: tokio::sync::mpsc::Sender<Intent>) {
        use std::collections::HashSet;
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(8))
            .build()
            .unwrap_or_default();
        let mut seen: HashSet<i64> = HashSet::new();

        loop {
            for &dst_chain in &self.dst_chains {
                let url = format!(
                    "https://app.across.to/api/deposits?status=unfilled&destinationChainId={}&limit={}",
                    dst_chain, self.limit
                );
                let Ok(resp) = client.get(&url).send().await else {
                    continue;
                };
                let Ok(deps) = resp.json::<Vec<serde_json::Value>>().await else {
                    continue;
                };

                let now_secs = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as i64;

                for dep in deps {
                    let dep_id = match dep.get("depositId").and_then(|v| v.as_i64()) {
                        Some(id) => id,
                        None => continue,
                    };
                    if !seen.insert(dep_id) {
                        continue;
                    }

                    // Exclusivity check: skip if exclusiveRelayer is set and deadline hasn't passed
                    let excl = dep.get("exclusiveRelayer")
                        .and_then(|v| v.as_str())
                        .unwrap_or("0x0000000000000000000000000000000000000000");
                    let is_exclusive = !excl.is_empty()
                        && excl != "0x0000000000000000000000000000000000000000";
                    if is_exclusive {
                        let excl_deadline = dep.get("exclusivityDeadline")
                            .and_then(|v| v.as_str())
                            .and_then(|s| chrono_unix_from_iso(s))
                            .unwrap_or(0);
                        if excl_deadline > now_secs {
                            continue; // still exclusive
                        }
                    }

                    // Fill deadline: skip if < 60s remaining
                    let fill_deadline_unix = dep.get("fillDeadline")
                        .and_then(|v| v.as_str())
                        .and_then(|s| chrono_unix_from_iso(s))
                        .unwrap_or(0);
                    if fill_deadline_unix > 0 && fill_deadline_unix - now_secs < 60 {
                        continue;
                    }

                    let intent = across_deposit_to_intent(&dep, dst_chain, dep_id, fill_deadline_unix);
                    info!("📡 AcrossPoller: depositId={} {}→{} outAmt={}",
                        dep_id, intent.src_chain, intent.dst_chain, intent.amount);

                    if intent_tx.send(intent).await.is_err() {
                        return; // receiver dropped
                    }
                }

                // Stagger chain polls slightly to avoid hitting rate limits
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
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
    let src_chain = dep.get("originChainId").and_then(|v| v.as_u64()).unwrap_or(1);
    let depositor = dep.get("depositor").and_then(|v| v.as_str()).unwrap_or("0x0").to_string();
    let recipient = dep.get("recipient").and_then(|v| v.as_str())
        .unwrap_or(&depositor).to_string();
    let input_token = dep.get("inputToken").and_then(|v| v.as_str()).unwrap_or("0x0").to_string();
    let output_token = dep.get("outputToken").and_then(|v| v.as_str()).unwrap_or("0x0").to_string();
    let input_amount = dep.get("inputAmount").and_then(|v| v.as_str())
        .or_else(|| dep.get("inputAmount").and_then(|v| v.as_u64()).map(|_| "0"))
        .unwrap_or("0").to_string();
    let output_amount = dep.get("outputAmount").and_then(|v| v.as_str())
        .unwrap_or("0").to_string();
    let tx_hash = dep.get("depositTxHash").and_then(|v| v.as_str())
        .or_else(|| dep.get("txHash").and_then(|v| v.as_str()))
        .unwrap_or("0x").to_string();
    let excl_relayer = dep.get("exclusiveRelayer").and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let excl_deadline = dep.get("exclusivityDeadline")
        .and_then(|v| v.as_str())
        .and_then(|s| chrono_unix_from_iso(s))
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

    /// Subscribe to genome stream and send intents to channel
    pub async fn subscribe(&self, intent_tx: mpsc::Sender<Intent>) -> Result<()> {
        info!("🔌 Connecting to genome stream: {}", self.sse_url);

        loop {
            match self.subscribe_internal(&intent_tx).await {
                Ok(_) => {
                    warn!("Genome stream ended unexpectedly, reconnecting in 5s...");
                }
                Err(e) => {
                    error!("Genome stream error: {}, reconnecting in 5s...", e);
                }
            }

            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
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

        while let Some(chunk) = response.chunk().await? {
            let text = String::from_utf8_lossy(&chunk);
            buffer.push_str(&text);

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

        let mut event_type = None;
        let mut data = None;

        for line in event_text.lines() {
            if let Some(content) = line.strip_prefix("event: ") {
                event_type = Some(content.trim());
            } else if let Some(content) = line.strip_prefix("data: ") {
                data = Some(content.trim());
            }
        }

        // Accept both "genome" and "genome_entry" event types
        if event_type != Some("genome") && event_type != Some("genome_entry") {
            return None;
        }

        let data = data?;

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
            // Log unknown actions on proto/order entities — may reveal deBridge shapes.
            info!("🔎 UNKNOWN action='{}' entity='{}' protocol={:?} id={:?} addr={}",
                genome_event.action, genome_event.entity,
                genome_event.protocol, genome_event.id, genome_event.addr);
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
}
