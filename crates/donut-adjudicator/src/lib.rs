//! Donut adjudicator — signed attestations for adapter-owner inflow
//! redistribution.
//!
//! ## What this crate does
//!
//! The Spinner is the registered adapter owner with the upstream order
//! contract / fee distributor (see `api.taifoon.dev`). On each fill, the
//! upstream fee distributor routes a small per-fill **inflow** to the
//! Spinner's wallet as the adapter-owner share. The Spinner then
//! **redistributes that inflow internally** to three purposes:
//!
//! * **70%** → adapter_builder (whoever shipped the bridge integration)
//! * **20%** → adapter_reviewers (the open-mamba code-review agents
//!             registered for the adapter)
//! * **10%** → adapter_ecosystem (catch-all + fail-closed absorber)
//!
//! The `DonutAttestation` records this **internal redistribution** as a
//! signed, hash-chained receipt. It does NOT record the upstream
//! protocol-level fee split — that is handled independently by the
//! upstream fee distributor.
//!
//! ## Why off-chain?
//!
//! Two protocol families cannot have the redistribution enforced on-chain
//! by the Spinner alone:
//!
//! 1. **Mayan-Solana Swift** — the protocol pays the solver's EOA
//!    directly, there is no relayer hook to splice the internal
//!    redistribution into.
//! 2. **LiFi meta-router** — LiFi is itself an aggregator; sub-attribution
//!    happens inside their off-chain accounting.
//!
//! For those (and as an audit trail for every other path) we produce a
//! **signed off-chain attestation**: a structured, deterministically-
//! serialized record of the redistribution that the Spinner signs with
//! their EVM key. Attestations chain by `prev_hash` so the ledger is
//! tamper-evident.
//!
//! ## The redistribution math
//!
//! **Critical**: the donut base is `max(0, inflow_usd_micro)`. Negative
//! inflows (refunds, claw-backs) produce a zero donut.
//!
//! ```text
//! donut_take      = max(0, inflow) * split_num / split_den   // default 1/1 = 100%
//! adapter_builder = donut_take * 70 / 100
//! reviewers       = donut_take * 20 / 100                    (split equally at payout)
//! ecosystem       = donut_take - builder - reviewers         (absorbs residual)
//! ```

use alloy::primitives::Address;
use alloy::signers::local::PrivateKeySigner;
use alloy::signers::{Signature, SignerSync};
use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use executor::OutcomeRecord;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use tracing::{debug, warn};

// ── Constants ──────────────────────────────────────────────────────────────────

/// Scale factor: $1.00 USD = 1_000_000 micro-USD. Chosen so a single
/// fill's inflow fits comfortably in i64 (max ~$9.2T) and the lossy
/// truncation in `compute_redistribution_micro` is bounded to < $0.000001.
pub const MICRO_USD_PER_USD: i64 = 1_000_000;

/// Default numerator of the fraction of `inflow_usd_micro` that gets
/// redistributed. Defaults to 1/1 = 100% (every micro-USD of inflow is
/// redistributed). Lower fraction retains inflow as operational margin;
/// default redistributes all of it.
pub const DEFAULT_INFLOW_REDISTRIBUTION_NUM: i64 = 1;
pub const DEFAULT_INFLOW_REDISTRIBUTION_DEN: i64 = 1;

/// Internal redistribution constants. Uniform across all adapters this
/// Spinner runs. Tampering with these constants and signing produces an
/// attestation `verify()` will reject — the math is re-derived from
/// pinned `split_num/split_den` and asserted equal to the carried shares.
pub const DEFAULT_BUILDER_NUM: u32 = 70;
pub const DEFAULT_REVIEWERS_NUM: u32 = 20;
pub const DEFAULT_ECOSYSTEM_NUM: u32 = 10;
pub const DEFAULT_SPLIT_DEN: u32 = 100;

/// Stable purpose tags for recipient entries on a `DonutAttestation`.
/// Used as the map key as well as the `RecipientShare.purpose` field —
/// keep them in sync.
pub const PURPOSE_ADAPTER_BUILDER: &str = "adapter_builder";
pub const PURPOSE_ADAPTER_REVIEWERS: &str = "adapter_reviewers";
pub const PURPOSE_ADAPTER_ECOSYSTEM: &str = "adapter_ecosystem";

/// Zero hash used as the `prev_hash` of the very first attestation in a
/// Spinner's ledger.
pub const ZERO_HASH: &str =
    "0x0000000000000000000000000000000000000000000000000000000000000000";

/// Solana destination sentinel used in tests and as one of several recognised
/// Solana chain-id encodings (the executor itself uses `1399811149`).
pub const SOLANA_DST_SENTINEL: u64 = 0;
/// Pseudo chain-id used by the Solana protocol adapters (see
/// `crates/protocol-adapters-solana/src/wormhole_ntt.rs`).
pub const SOLANA_DST_WORMHOLE: u64 = 1_399_811_149;
/// Alternate Solana destination id used by deBridge poller integration.
pub const SOLANA_DST_DEBRIDGE: u64 = 100_000_001;

// ── Public types ───────────────────────────────────────────────────────────────

/// Maps adapter ids to the on-chain addresses that receive their share
/// of the internal redistribution. The Spinner OS loads this from a
/// config file at boot.
#[derive(Debug, Clone)]
pub struct AdapterRegistry {
    /// `adapter_id` → adapter_builder address (70% recipient).
    pub builders: HashMap<String, Address>,
    /// `adapter_id` → ordered list of reviewer addresses. The 20%
    /// reviewer pool is divided equally between them at payout time.
    pub reviewers: HashMap<String, Vec<Address>>,
    /// `adapter_id` → optional `(redist_num, redist_den)` override
    /// controlling what fraction of `inflow_usd_micro` gets redistributed
    /// (vs. retained as operational margin). When absent, the canonical
    /// default `(1, 1)` = 100% applies.
    pub bps_overrides: HashMap<String, (i64, i64)>,
    /// Single ecosystem treasury address. Receives the 10% adapter_ecosystem
    /// purpose AND, for unregistered adapters, both the builder and
    /// reviewer shares as the fail-closed absorber.
    pub ecosystem: Address,
}

impl AdapterRegistry {
    pub fn new(ecosystem: Address) -> Self {
        Self {
            builders: HashMap::new(),
            reviewers: HashMap::new(),
            bps_overrides: HashMap::new(),
            ecosystem,
        }
    }

    pub fn with_adapter(
        mut self,
        adapter_id: impl Into<String>,
        builder: Address,
        reviewers: Vec<Address>,
    ) -> Self {
        let id: String = adapter_id.into();
        self.builders.insert(id.clone(), builder);
        self.reviewers.insert(id, reviewers);
        self
    }

    /// Same as `with_adapter` but also pins a per-adapter inflow-
    /// redistribution fraction. Use for adapters that retain a portion
    /// of their inflow as operational margin instead of redistributing
    /// 100% of it.
    pub fn with_adapter_bps(
        mut self,
        adapter_id: impl Into<String>,
        builder: Address,
        reviewers: Vec<Address>,
        bps_num: i64,
        bps_den: i64,
    ) -> Self {
        let id: String = adapter_id.into();
        self.builders.insert(id.clone(), builder);
        self.reviewers.insert(id.clone(), reviewers);
        self.bps_overrides.insert(id, (bps_num, bps_den));
        self
    }

    /// Effective inflow-redistribution fraction `(num, den)` for an
    /// adapter — override if registered, canonical default `(1, 1)`
    /// otherwise. Method name retained for back-compat with callers that
    /// pre-date the inflow framing.
    pub fn bps_for(&self, adapter_id: &str) -> (i64, i64) {
        self.bps_overrides
            .get(adapter_id)
            .copied()
            .unwrap_or((
                DEFAULT_INFLOW_REDISTRIBUTION_NUM,
                DEFAULT_INFLOW_REDISTRIBUTION_DEN,
            ))
    }
}

/// Signed, deterministically-hashed receipt of how the Spinner
/// redistributed an upstream adapter-owner inflow on a single fill.
/// Hash-chained per Spinner.
///
/// Every field is `#[serde]`-stable. Canonical JSON for hashing/signing
/// is produced by [`canonical_json_for_signing`] (signature field
/// omitted) and [`canonical_json_with_signature`] (signature field
/// included — used for chaining the next `prev_hash`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DonutAttestation {
    pub fill_id: String,
    pub spinner_id: String,
    pub spinner_addr: Address,
    pub adapter_id: String,
    pub protocol: String,
    pub dst_chain: u64,

    /// Fee component decoded from the SSE intent / upstream order. The
    /// Spinner receives this as gross revenue from filling. Distinct
    /// from `inflow_usd_micro`: the fee is what the *protocol* pays the
    /// Spinner, the inflow is what the *upstream adapter registry*
    /// routes back as the adapter-owner share.
    pub fee_usd_micro: i64,

    /// Realised net profit for the Spinner (fee minus gas). Tracks the
    /// Spinner's executor margin and is NOT the base for the donut math
    /// below — that's `inflow_usd_micro`. Can be negative on losses.
    pub actual_profit_usd_micro: i64,

    /// Inflow the Spinner's wallet receives from the upstream adapter
    /// registry for being the adapter owner on this fill, in micro-USD.
    /// This is the amount the `recipients` map below redistributes.
    pub inflow_usd_micro: i64,

    /// Numerator of the fraction of `inflow_usd_micro` that gets
    /// redistributed via `recipients`. Default `(1, 1)` = 100% of inflow.
    /// An adapter that retains operational margin would set this lower.
    pub split_num: i64,
    pub split_den: i64,

    /// Total amount distributed across `recipients`. Equals
    /// `max(0, inflow_usd_micro) × split_num / split_den` exactly.
    pub donut_take_usd_micro: i64,

    /// Internal redistribution of `donut_take_usd_micro`. Keyed by
    /// stable purpose tag — see `PURPOSE_*` constants. The sum of every
    /// `RecipientShare.share_usd_micro` equals `donut_take_usd_micro`
    /// exactly; the recipient with `is_residual: true` absorbs the
    /// integer-division residual.
    pub recipients: BTreeMap<String, RecipientShare>,

    pub ts: DateTime<Utc>,
    /// sha256 of the previous attestation's canonical JSON *with* its
    /// signature. Hex with `0x` prefix. Zero-hash for the first
    /// attestation.
    pub prev_hash: String,
    /// EIP-191 personal_sign over `canonical_json_for_signing(self)` as
    /// bytes. 0x-prefixed 65-byte hex (r || s || v).
    pub signature_hex: String,
}

/// One entry in `DonutAttestation::recipients`. Self-describing — the
/// `purpose` field equals the map key.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RecipientShare {
    /// Stable purpose tag (also the map key).
    pub purpose: String,
    /// Addresses that receive this share. When >1, the amount is
    /// divided equally across them at payout.
    pub addresses: Vec<Address>,
    /// Amount in micro-USD allocated to this purpose (total, before
    /// equal split across `addresses` at payout time).
    pub share_usd_micro: i64,
    /// Numerator of the share fraction (e.g. 70).
    pub share_num: u32,
    /// Denominator (typically 100).
    pub share_den: u32,
    /// Exactly one recipient per attestation has this true. It absorbs
    /// the integer-division residual so the sum across all recipients
    /// equals `donut_take_usd_micro` to the cent.
    pub is_residual: bool,
}

/// Behaviour expected of any donut adjudicator implementation.
#[async_trait]
pub trait FeeSplitAdjudicator: Send + Sync {
    /// Produce a signed [`DonutAttestation`] from a fill outcome.
    async fn attest(
        &self,
        fill: &OutcomeRecord,
        registry: &AdapterRegistry,
        signer: &PrivateKeySigner,
        prev_hash: &str,
    ) -> Result<DonutAttestation>;

    /// Validate an attestation's signature and internal math.
    fn verify(&self, att: &DonutAttestation) -> Result<()>;
}

/// Canonical implementation of [`FeeSplitAdjudicator`].
#[derive(Debug, Default, Clone, Copy)]
pub struct CanonicalAdjudicator;

// ── adapter id derivation ──────────────────────────────────────────────────────

/// Returns `true` for chain ids that this crate recognises as Solana
/// **destinations**.
///
/// The Solana mainnet cluster is encoded several ways in the upstream
/// integrations:
///
/// * `1_399_811_149` — Wormhole/Mayan canonical Solana chain id, written to
///   `OutcomeRecord.dst_chain` by the Mayan-Solana broadcaster path
///   (`crates/genome-client/src/lib.rs:1770-1772`).
/// * `100_000_001` — deBridge DLN Solana sentinel emitted by the DLN poller
///   (`crates/genome-client/src/lib.rs:863,1395-1397`).
/// * `0` — defensive sentinel used by some in-process unit tests; no live
///   emitter writes this.
pub fn is_solana_dst(dst_chain: u64) -> bool {
    dst_chain == SOLANA_DST_SENTINEL
        || dst_chain == SOLANA_DST_WORMHOLE
        || dst_chain == SOLANA_DST_DEBRIDGE
}

/// Returns `true` when EITHER `src_chain` OR `dst_chain` resolves to Solana.
///
/// Why both: the Mayan Swift Solana program also handles the
/// **Solana-source → EVM-destination** redeem path (VAA-redeem on EVM is
/// driven by the Solana initialise_order). The work — and therefore the
/// adapter_builder credit — is owned by the Solana program author even
/// though the fill itself broadcasts to an EVM destination. Keying the
/// adapter id off only `dst_chain` mis-attributes that flow.
pub fn is_solana_involved(src_chain: u64, dst_chain: u64) -> bool {
    is_solana_dst(src_chain) || is_solana_dst(dst_chain)
}

/// Map `(protocol, dst_chain)` → adapter id used to look up adapter_builder
/// / reviewer addresses in an [`AdapterRegistry`].
///
/// **Prefer [`adapter_id_for_outcome`]** when you have access to the full
/// `OutcomeRecord` — it uses both `src_chain` and `dst_chain` and correctly
/// attributes Solana-source-EVM-destination flows to the Solana builder.
/// This 2-arg variant is kept for callers that only know the destination.
pub fn default_adapter_id(protocol: &str, dst_chain: u64) -> String {
    // Use dst_chain as src_chain fallback (2-arg legacy mode) — prevents
    // chain 0 (SOLANA_DST_SENTINEL) from triggering Solana routing when
    // the actual source is unknown but destination is EVM.
    adapter_id_resolve(protocol, dst_chain, dst_chain)
}

/// Map `(protocol, src_chain, dst_chain)` from an [`OutcomeRecord`] → adapter id.
pub fn adapter_id_for_outcome(record: &OutcomeRecord) -> String {
    adapter_id_resolve(&record.protocol, record.src_chain, record.dst_chain)
}

fn adapter_id_resolve(protocol: &str, src_chain: u64, dst_chain: u64) -> String {
    let p = protocol.to_ascii_lowercase();
    let solana = is_solana_involved(src_chain, dst_chain);

    // Mayan Flash **must** be matched before the generic Mayan branch
    // because it's a separate Anchor program with a separate adapter_builder.
    if p == "mayan_flash" || p.contains("flash") && p.contains("mayan") {
        return if solana {
            "mayan-flash-solana-v1".to_string()
        } else {
            "mayan-flash-evm-v1".to_string()
        };
    }
    // Mayan Swift family (also catches the legacy bare `"mayan"`).
    if p.starts_with("mayan") {
        return if solana {
            "mayan-solana-swift-v1".to_string()
        } else {
            "mayan-evm-swift-v1".to_string()
        };
    }
    // Wormhole NTT — protocol strings observed: `wormhole_ntt`, `wormhole`, `ntt`.
    if p.contains("wormhole") || p == "ntt" || p.contains("ntt") {
        return "wormhole-ntt-solana-v1".to_string();
    }
    if p == "lifi" {
        return "lifi-meta-v2".to_string();
    }
    if p == "across" {
        return "across-v3".to_string();
    }
    if p == "debridge" || p == "dln" || p.contains("debridge") || p.contains("dln") {
        return if solana {
            "debridge-dln-solana-v1".to_string()
        } else {
            "debridge-dln-v1".to_string()
        };
    }
    format!("unknown-{}-{}", p, dst_chain)
}

// ── Canonical JSON ─────────────────────────────────────────────────────────────

/// Recursively re-sort every `Object` so the output is byte-stable regardless
/// of insertion order. Arrays preserve their order. Numbers / strings / nulls
/// pass through unchanged.
fn sort_value(v: serde_json::Value) -> serde_json::Value {
    use serde_json::Value;
    match v {
        Value::Object(map) => {
            let sorted: BTreeMap<String, Value> =
                map.into_iter().map(|(k, v)| (k, sort_value(v))).collect();
            // serde_json::Map preserves insertion order; feeding from a BTreeMap
            // gives lexicographic key order in the resulting Map.
            let mut out = serde_json::Map::with_capacity(sorted.len());
            for (k, v) in sorted {
                out.insert(k, v);
            }
            Value::Object(out)
        }
        Value::Array(items) => Value::Array(items.into_iter().map(sort_value).collect()),
        other => other,
    }
}

/// Canonical JSON used as the signing pre-image. Excludes the `signature_hex`
/// field so a freshly-built attestation can compute its own signature without
/// chicken-and-egg.
pub fn canonical_json_for_signing(att: &DonutAttestation) -> Result<String> {
    let mut v = serde_json::to_value(att).context("serialize attestation")?;
    if let serde_json::Value::Object(ref mut map) = v {
        map.remove("signature_hex");
    }
    let sorted = sort_value(v);
    serde_json::to_string(&sorted).context("serialize canonical json")
}

/// Canonical JSON *including* the signature. This is the pre-image that feeds
/// the next attestation's `prev_hash` so tampering with a signed record breaks
/// the chain.
pub fn canonical_json_with_signature(att: &DonutAttestation) -> Result<String> {
    let v = serde_json::to_value(att).context("serialize attestation")?;
    let sorted = sort_value(v);
    serde_json::to_string(&sorted).context("serialize canonical json")
}

/// sha256 of the canonical JSON (with signature). Returned as `0x`-prefixed
/// lowercase hex. Drives the [`DonutAttestation::prev_hash`] chain.
pub fn hash_for_chain(att: &DonutAttestation) -> Result<String> {
    let json = canonical_json_with_signature(att)?;
    let mut h = Sha256::new();
    h.update(json.as_bytes());
    let digest = h.finalize();
    Ok(format!("0x{}", hex::encode(digest)))
}

// ── Math ───────────────────────────────────────────────────────────────────────

/// Pure micro-USD math — no signing, no I/O. Returns
/// `(donut_take, builder, reviewers, ecosystem)` in micro-USD. The
/// ecosystem share absorbs the integer-division residual so the three
/// recipient shares sum to `donut_take` exactly.
pub fn compute_redistribution_micro(
    inflow_usd_micro: i64,
    split_num: i64,
    split_den: i64,
) -> (i64, i64, i64, i64) {
    let positive = inflow_usd_micro.max(0);
    let donut = positive * split_num / split_den;
    let builder = donut * (DEFAULT_BUILDER_NUM as i64) / (DEFAULT_SPLIT_DEN as i64);
    let reviewers = donut * (DEFAULT_REVIEWERS_NUM as i64) / (DEFAULT_SPLIT_DEN as i64);
    let ecosystem = donut - builder - reviewers; // residual
    (donut, builder, reviewers, ecosystem)
}

/// Convenience wrapper: redistribute 100% of inflow using the canonical
/// default `(1, 1)` fraction. Mirrors the back-compat call site signature
/// used by tests that pre-date per-adapter overrides.
pub fn compute_redistribution_micro_default(
    inflow_usd_micro: i64,
) -> (i64, i64, i64, i64) {
    compute_redistribution_micro(
        inflow_usd_micro,
        DEFAULT_INFLOW_REDISTRIBUTION_NUM,
        DEFAULT_INFLOW_REDISTRIBUTION_DEN,
    )
}

/// Convert a USD float to micro-USD i64. Rounded to nearest; non-finite
/// inputs become 0. Used at the f64-input boundary (executor's
/// `OutcomeRecord.actual_profit_usd: Option<f64>`).
pub fn usd_to_micro(usd: f64) -> i64 {
    if !usd.is_finite() {
        return 0;
    }
    (usd * MICRO_USD_PER_USD as f64).round() as i64
}

// ── Adapter registry loader ────────────────────────────────────────────────────

/// Public donut-policy constants. Exposed by `GET /api/donut/policy` so
/// any auditor can confirm the canonical redistribution is uniform
/// across every adapter and every Spinner. The values here are the SAME
/// constants `compute_redistribution_micro` uses internally — no second
/// source of truth.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DonutPolicy {
    /// Numerator of the inflow-redistribution fraction (default `1`).
    pub split_num: i64,
    /// Denominator of the inflow-redistribution fraction (default `1`).
    pub split_den: i64,
    pub builder_num: i64,
    pub reviewers_num: i64,
    pub ecosystem_num: i64,
    /// Denominator of the share-fraction triple (typically `100`).
    pub split_share_den: i64,
    /// `1_000_000` — every `_usd_micro` field is this many micro-USD per
    /// USD.
    pub micro_usd_per_usd: i64,
    /// Applies-to scope.
    pub applies_to: String,
    /// Adjudicator implementation version. Increments whenever the math
    /// or canonical-JSON layout changes.
    pub adjudicator_version: String,
}

impl DonutPolicy {
    /// Canonical values currently in force. If you change any of these,
    /// bump `adjudicator_version`.
    pub fn canonical() -> Self {
        Self {
            // Default = 100% of inflow redistributed.
            split_num: DEFAULT_INFLOW_REDISTRIBUTION_NUM,
            split_den: DEFAULT_INFLOW_REDISTRIBUTION_DEN,
            // Internal redistribution — uniform across all adapters.
            builder_num: DEFAULT_BUILDER_NUM as i64,
            reviewers_num: DEFAULT_REVIEWERS_NUM as i64,
            ecosystem_num: DEFAULT_ECOSYSTEM_NUM as i64,
            split_share_den: DEFAULT_SPLIT_DEN as i64,
            micro_usd_per_usd: MICRO_USD_PER_USD,
            applies_to: "all_provisioned_adapter_inflows".to_string(),
            adjudicator_version: "canonical-v2".to_string(),
        }
    }
}

/// Public view of an [`AdapterRegistry`] — same data, serializable. Used
/// by `GET /api/donut/registry` so anyone can confirm the
/// adapter → builder mapping a Spinner claims to apply.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdapterRegistryView {
    pub ecosystem: Address,
    pub adapters: BTreeMap<String, AdapterRegistryEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdapterRegistryEntry {
    pub builder: Address,
    pub reviewers: Vec<Address>,
    /// Per-adapter inflow-redistribution numerator. When set, overrides
    /// the canonical default `1`. Use the `bps()` helper to read the
    /// effective rate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub donut_bps_num: Option<i64>,
    /// Per-adapter inflow-redistribution denominator. Defaults to `1`
    /// when absent. Both `num` and `den` must be set together for an
    /// override to apply.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub donut_bps_den: Option<i64>,
}

impl AdapterRegistryEntry {
    /// Effective `(numerator, denominator)` for this adapter's
    /// inflow-redistribution fraction. Falls back to the canonical
    /// default when the entry doesn't override.
    pub fn bps(&self) -> (i64, i64) {
        match (self.donut_bps_num, self.donut_bps_den) {
            (Some(n), Some(d)) if d > 0 => (n, d),
            _ => (
                DEFAULT_INFLOW_REDISTRIBUTION_NUM,
                DEFAULT_INFLOW_REDISTRIBUTION_DEN,
            ),
        }
    }
}

impl AdapterRegistry {
    /// Serializable view for the public `/api/donut/registry` endpoint.
    /// Sorts adapter ids so the output is byte-stable across calls.
    pub fn view(&self) -> AdapterRegistryView {
        let mut adapters: BTreeMap<String, AdapterRegistryEntry> = BTreeMap::new();
        for (id, builder) in &self.builders {
            let reviewers = self.reviewers.get(id).cloned().unwrap_or_default();
            let (num, den) = self
                .bps_overrides
                .get(id)
                .copied()
                .map(|(n, d)| (Some(n), Some(d)))
                .unwrap_or((None, None));
            adapters.insert(
                id.clone(),
                AdapterRegistryEntry {
                    builder: *builder,
                    reviewers,
                    donut_bps_num: num,
                    donut_bps_den: den,
                },
            );
        }
        AdapterRegistryView {
            ecosystem: self.ecosystem,
            adapters,
        }
    }

    /// Load from a JSON file. Schema mirrors `AdapterRegistryView`:
    ///
    /// ```json
    /// {
    ///   "ecosystem": "0x...",
    ///   "adapters": {
    ///     "mayan-solana-swift-v1": {
    ///       "builder": "0x...",
    ///       "reviewers": ["0x...", "0x..."]
    ///     }
    ///   }
    /// }
    /// ```
    ///
    /// On missing/unreadable file, returns a registry with the ZERO
    /// address as `ecosystem` and zero builders — every fill then routes
    /// the builder + reviewer shares to that ZERO address (fail-closed).
    /// The caller is expected to log a loud warning when this happens.
    pub fn load_from_path(path: &std::path::Path) -> Result<Self> {
        let raw = std::fs::read_to_string(path)
            .with_context(|| format!("read adapter_registry from {}", path.display()))?;
        let view: AdapterRegistryView = serde_json::from_str(&raw)
            .with_context(|| format!("parse adapter_registry JSON at {}", path.display()))?;
        let mut reg = AdapterRegistry::new(view.ecosystem);
        for (id, entry) in view.adapters {
            match entry.bps() {
                (n, d)
                    if (n, d)
                        != (
                            DEFAULT_INFLOW_REDISTRIBUTION_NUM,
                            DEFAULT_INFLOW_REDISTRIBUTION_DEN,
                        ) =>
                {
                    reg = reg.with_adapter_bps(id, entry.builder, entry.reviewers, n, d);
                }
                _ => {
                    reg = reg.with_adapter(id, entry.builder, entry.reviewers);
                }
            }
        }
        Ok(reg)
    }

    /// Same as `load_from_path` but consumes the `ADAPTER_REGISTRY_PATH`
    /// environment variable (default `./config/adapter_registry.json`)
    /// and falls back to an **empty** registry with the ZERO ecosystem
    /// address when the file is missing — never panics, never silently
    /// pockets the inflow.
    pub fn load_default() -> Self {
        let path = std::env::var("ADAPTER_REGISTRY_PATH")
            .unwrap_or_else(|_| "./config/adapter_registry.json".to_string());
        let p = std::path::Path::new(&path);
        match Self::load_from_path(p) {
            Ok(reg) => {
                tracing::info!(
                    "loaded adapter_registry from {} ({} adapters)",
                    path,
                    reg.builders.len()
                );
                reg
            }
            Err(e) => {
                warn!(
                    "adapter_registry not loadable from {} ({}); falling back to fail-closed empty registry (every donut share routes to ZERO ecosystem address — fix this before going live)",
                    path, e
                );
                AdapterRegistry::new(Address::ZERO)
            }
        }
    }
}

// ── CanonicalAdjudicator impl ──────────────────────────────────────────────────

#[async_trait]
impl FeeSplitAdjudicator for CanonicalAdjudicator {
    async fn attest(
        &self,
        fill: &OutcomeRecord,
        registry: &AdapterRegistry,
        signer: &PrivateKeySigner,
        prev_hash: &str,
    ) -> Result<DonutAttestation> {
        // The inflow IS the SSE-decoded fee under the current
        // arrangement (Spinner is the registered adapter owner and the
        // fee component routes to the Spinner's wallet via the upstream
        // registry). When the executor can't decode a fee, fall back to
        // actual_profit (a conservative substitute that never
        // over-takes).
        let inflow_usd_f = fill.fee_usd.or(fill.actual_profit_usd).unwrap_or(0.0);
        let inflow_micro = usd_to_micro(inflow_usd_f);
        let profit_micro = usd_to_micro(fill.actual_profit_usd.unwrap_or(0.0));
        let fee_micro = usd_to_micro(fill.fee_usd.unwrap_or(0.0));

        let adapter_id = adapter_id_for_outcome(fill);
        let ecosystem_addr = registry.ecosystem;

        // Per-adapter inflow-redistribution fraction. Falls back to the
        // canonical default (1/1 = 100%) when no override is registered.
        let (split_num, split_den) = registry.bps_for(&adapter_id);
        let (donut, builder, reviewers, ecosystem) =
            compute_redistribution_micro(inflow_micro, split_num, split_den);

        // Fail-closed: unknown adapter → route the builder + reviewer
        // cuts to the ecosystem treasury. Spinners running unregistered
        // adapters do NOT silently pocket the builder's share.
        let (builder_addr, reviewer_addrs) = match registry.builders.get(&adapter_id) {
            Some(addr) => {
                let revs = registry
                    .reviewers
                    .get(&adapter_id)
                    .cloned()
                    .unwrap_or_default();
                (*addr, revs)
            }
            None => {
                warn!(
                    adapter_id = %adapter_id,
                    "unknown adapter — routing builder + reviewer shares to ecosystem"
                );
                (ecosystem_addr, vec![ecosystem_addr])
            }
        };

        let mut recipients: BTreeMap<String, RecipientShare> = BTreeMap::new();
        recipients.insert(
            PURPOSE_ADAPTER_BUILDER.to_string(),
            RecipientShare {
                purpose: PURPOSE_ADAPTER_BUILDER.into(),
                addresses: vec![builder_addr],
                share_usd_micro: builder,
                share_num: DEFAULT_BUILDER_NUM,
                share_den: DEFAULT_SPLIT_DEN,
                is_residual: false,
            },
        );
        recipients.insert(
            PURPOSE_ADAPTER_REVIEWERS.to_string(),
            RecipientShare {
                purpose: PURPOSE_ADAPTER_REVIEWERS.into(),
                addresses: reviewer_addrs,
                share_usd_micro: reviewers,
                share_num: DEFAULT_REVIEWERS_NUM,
                share_den: DEFAULT_SPLIT_DEN,
                is_residual: false,
            },
        );
        recipients.insert(
            PURPOSE_ADAPTER_ECOSYSTEM.to_string(),
            RecipientShare {
                purpose: PURPOSE_ADAPTER_ECOSYSTEM.into(),
                addresses: vec![ecosystem_addr],
                share_usd_micro: ecosystem,
                share_num: DEFAULT_ECOSYSTEM_NUM,
                share_den: DEFAULT_SPLIT_DEN,
                is_residual: true,
            },
        );

        let spinner_addr = signer.address();
        let spinner_id = short_id(&spinner_addr);
        let fill_id = derive_fill_id(fill);

        // Build the attestation with a placeholder signature so we can produce
        // the canonical pre-image, then fill in the signature.
        let mut att = DonutAttestation {
            fill_id,
            spinner_id,
            spinner_addr,
            adapter_id,
            protocol: fill.protocol.clone(),
            dst_chain: fill.dst_chain,
            fee_usd_micro: fee_micro,
            actual_profit_usd_micro: profit_micro,
            inflow_usd_micro: inflow_micro,
            split_num,
            split_den,
            donut_take_usd_micro: donut,
            recipients,
            ts: fill.ts,
            prev_hash: prev_hash.to_string(),
            signature_hex: String::new(),
        };

        let canonical = canonical_json_for_signing(&att)?;
        // `sign_message` is `SignerSync` for `PrivateKeySigner` — synchronous,
        // works inside an `async fn` without blocking the runtime.
        let sig: Signature = signer
            .sign_message_sync(canonical.as_bytes())
            .context("sign attestation")?;
        att.signature_hex = format!("0x{}", hex::encode(sig.as_bytes()));

        debug!(
            spinner = %att.spinner_addr,
            adapter = %att.adapter_id,
            donut_usd_micro = att.donut_take_usd_micro,
            "attestation signed"
        );

        Ok(att)
    }

    fn verify(&self, att: &DonutAttestation) -> Result<()> {
        // 1. Redistribution math is internally consistent — integer-exact.
        if att.split_den <= 0 || att.split_num < 0 {
            return Err(anyhow!(
                "invalid inflow redistribution fraction: {}/{}",
                att.split_num,
                att.split_den
            ));
        }
        let positive_inflow = att.inflow_usd_micro.max(0);
        let expected_donut = positive_inflow * att.split_num / att.split_den;
        if att.donut_take_usd_micro != expected_donut {
            return Err(anyhow!(
                "donut_take_usd_micro mismatch: got {}, expected {} (inflow={} × {}/{})",
                att.donut_take_usd_micro,
                expected_donut,
                positive_inflow,
                att.split_num,
                att.split_den
            ));
        }

        // 2. Recipient sum equals donut_take exactly, and exactly one
        //    recipient has `is_residual: true`.
        let mut sum: i64 = 0;
        let mut residual_count: usize = 0;
        for (key, share) in &att.recipients {
            if key != &share.purpose {
                return Err(anyhow!(
                    "recipient map key `{}` does not match purpose `{}`",
                    key,
                    share.purpose
                ));
            }
            sum = sum
                .checked_add(share.share_usd_micro)
                .ok_or_else(|| anyhow!("recipient share sum overflow"))?;
            if share.is_residual {
                residual_count += 1;
            }
        }
        if sum != att.donut_take_usd_micro {
            return Err(anyhow!(
                "share sum mismatch: {} != donut_take {}",
                sum,
                att.donut_take_usd_micro
            ));
        }
        if residual_count != 1 {
            return Err(anyhow!(
                "exactly one residual recipient required (found {})",
                residual_count
            ));
        }

        // 3. Signature recovers to `spinner_addr`.
        let canonical = canonical_json_for_signing(att)?;
        let sig_hex = att
            .signature_hex
            .strip_prefix("0x")
            .unwrap_or(&att.signature_hex);
        let sig_bytes = hex::decode(sig_hex).context("decode signature hex")?;
        if sig_bytes.len() != 65 {
            return Err(anyhow!(
                "signature must be 65 bytes, got {}",
                sig_bytes.len()
            ));
        }
        let sig = Signature::try_from(sig_bytes.as_slice()).context("parse signature")?;

        // Reject high-s signatures (signature malleability). secp256k1's
        // group order n = 0xFFFFFFFF…BFD25E8CD0364141; the "low half" is
        // s <= n/2. Allowing the high half means an attacker can flip
        // s → n-s on a known signature and produce a *different* 65-byte
        // sequence that still recovers the same address — which would
        // break ledger fingerprinting if `signature_hex` were used as a
        // dedup key. We inspect the raw bytes (alloy 0.8's `Signature::s()`
        // surface varies across patch releases) to stay independent of
        // crate-internal API changes.
        //
        // n/2 = 0x7FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF5D576E7357A4501DDFE92F46681B20A0
        const N_HALF_BE: [u8; 32] = [
            0x7F, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
            0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
            0x5D, 0x57, 0x6E, 0x73, 0x57, 0xA4, 0x50, 0x1D,
            0xDF, 0xE9, 0x2F, 0x46, 0x68, 0x1B, 0x20, 0xA0,
        ];
        let s_bytes = &sig_bytes[32..64];
        // big-endian unsigned compare via slice ordering.
        if s_bytes > &N_HALF_BE[..] {
            return Err(anyhow!(
                "signature s value is in high half (malleability prevented)"
            ));
        }

        let recovered = sig
            .recover_address_from_msg(canonical.as_bytes())
            .context("recover address from signature")?;
        if recovered != att.spinner_addr {
            return Err(anyhow!(
                "signature recovered {} but attestation claims spinner {}",
                recovered,
                att.spinner_addr
            ));
        }

        Ok(())
    }
}

// ── Helpers ────────────────────────────────────────────────────────────────────

/// `solver_id` convention used elsewhere in the repo — first 8 hex chars of
/// the address (lowercase, no `0x`). Public so callers (persistence layer,
/// HTTP handler) can derive the expected `spinner_id` from a recovered
/// address and reject attestations whose body claims a different id.
pub fn spinner_id_from_addr(addr: &Address) -> String {
    let hex = format!("{:x}", addr);
    hex.chars().take(8).collect()
}

/// Back-compat alias used inside this crate.
fn short_id(addr: &Address) -> String {
    spinner_id_from_addr(addr)
}

/// `OutcomeRecord` has no stable `fill_id` field, so we synthesise one from
/// `(intent_id, tx_hash_or_ts)`. Using `tx_hash` when present keeps the id
/// stable across replays; falling back to the timestamp avoids collisions for
/// skip rows that share an intent id.
pub fn derive_fill_id(rec: &OutcomeRecord) -> String {
    match rec.tx_hash.as_deref() {
        Some(h) if !h.is_empty() => format!("{}:{}", rec.intent_id, h),
        _ => format!(
            "{}:{}",
            rec.intent_id,
            rec.ts.timestamp_nanos_opt().unwrap_or(0)
        ),
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_signer() -> PrivateKeySigner {
        // Deterministic dev key — never used outside the test process.
        "0x4c0883a69102937d6231471b5dbb6204fe5129617082792ae468d01a3f362318"
            .parse()
            .unwrap()
    }

    fn make_signer_b() -> PrivateKeySigner {
        "0xdf57089febbacf7ba0bc227dafbffa9fc08a93fdc67e1e3c5ac6739c1bff21bd"
            .parse()
            .unwrap()
    }

    fn ecosystem_addr() -> Address {
        "0x000000000000000000000000000000000000eeee".parse().unwrap()
    }

    fn builder_addr() -> Address {
        "0x000000000000000000000000000000000000bbbb".parse().unwrap()
    }

    fn reviewer_addrs() -> Vec<Address> {
        vec![
            "0x0000000000000000000000000000000000000aaa".parse().unwrap(),
            "0x000000000000000000000000000000000000aaab".parse().unwrap(),
        ]
    }

    fn registry_with_known_adapter(adapter: &str) -> AdapterRegistry {
        AdapterRegistry::new(ecosystem_addr()).with_adapter(
            adapter,
            builder_addr(),
            reviewer_addrs(),
        )
    }

    fn outcome(protocol: &str, dst_chain: u64, profit: f64) -> OutcomeRecord {
        OutcomeRecord {
            ts: Utc::now(),
            intent_id: "intent-test-1".into(),
            protocol: protocol.into(),
            src_chain: 1,
            dst_chain,
            decision: "executed".into(),
            tx_hash: Some("0xabc".into()),
            predicted_gas: None,
            gas_used: Some(100_000),
            effective_gas_price_wei: None,
            predicted_profit_usd: Some(profit),
            actual_profit_usd: Some(profit),
            skip_reason: None,
            error: None,
            solver_id: Some("00000000".into()),
            claim_tx_hash: None,
            claim_fee_usd: None,
            fee_usd: None,
        }
    }

    fn builder_share(att: &DonutAttestation) -> i64 {
        att.recipients
            .get(PURPOSE_ADAPTER_BUILDER)
            .map(|r| r.share_usd_micro)
            .unwrap_or(0)
    }

    fn reviewers_share(att: &DonutAttestation) -> i64 {
        att.recipients
            .get(PURPOSE_ADAPTER_REVIEWERS)
            .map(|r| r.share_usd_micro)
            .unwrap_or(0)
    }

    fn ecosystem_share(att: &DonutAttestation) -> i64 {
        att.recipients
            .get(PURPOSE_ADAPTER_ECOSYSTEM)
            .map(|r| r.share_usd_micro)
            .unwrap_or(0)
    }

    fn builder_addr_of(att: &DonutAttestation) -> Address {
        att.recipients
            .get(PURPOSE_ADAPTER_BUILDER)
            .and_then(|r| r.addresses.first().copied())
            .unwrap_or(Address::ZERO)
    }

    fn reviewer_addrs_of(att: &DonutAttestation) -> Vec<Address> {
        att.recipients
            .get(PURPOSE_ADAPTER_REVIEWERS)
            .map(|r| r.addresses.clone())
            .unwrap_or_default()
    }

    #[tokio::test]
    async fn losing_fill_emits_zero_donut() {
        let adj = CanonicalAdjudicator;
        let signer = make_signer();
        let reg = registry_with_known_adapter("across-v3");
        let fill = outcome("across", 42161, -1.0);

        let att = adj.attest(&fill, &reg, &signer, ZERO_HASH).await.unwrap();
        assert_eq!(att.donut_take_usd_micro, 0);
        assert_eq!(builder_share(&att), 0);
        assert_eq!(reviewers_share(&att), 0);
        assert_eq!(ecosystem_share(&att), 0);
        // -1.0 USD = -1_000_000 micro-USD; profit carries the loss.
        assert_eq!(att.actual_profit_usd_micro, -1_000_000);
        adj.verify(&att).unwrap();
    }

    #[tokio::test]
    async fn winning_fill_math_sums_exactly() {
        let adj = CanonicalAdjudicator;
        let signer = make_signer();
        let reg = registry_with_known_adapter("across-v3");
        let fill = outcome("across", 42161, 100.0);

        let att = adj.attest(&fill, &reg, &signer, ZERO_HASH).await.unwrap();
        // 100.0 USD = 100_000_000 micro-USD inflow (fee_usd defaults to
        // profit when None — see attest()). Donut = 100_000_000 * 1/1.
        assert_eq!(att.inflow_usd_micro, 100_000_000);
        assert_eq!(att.donut_take_usd_micro, 100_000_000);
        let sum = builder_share(&att) + reviewers_share(&att) + ecosystem_share(&att);
        assert_eq!(sum, att.donut_take_usd_micro);
        // 70/20/10 split: 70_000_000 / 20_000_000 / 10_000_000.
        assert_eq!(builder_share(&att), 70_000_000);
        assert_eq!(reviewers_share(&att), 20_000_000);
        assert_eq!(ecosystem_share(&att), 10_000_000);
        adj.verify(&att).unwrap();
    }

    #[tokio::test]
    async fn mayan_solana_attestation() {
        let adj = CanonicalAdjudicator;
        let signer = make_signer();
        let reg = registry_with_known_adapter("mayan-solana-swift-v1");
        let fill = outcome("mayan", SOLANA_DST_SENTINEL, 2.50);

        let att = adj.attest(&fill, &reg, &signer, ZERO_HASH).await.unwrap();
        assert_eq!(att.adapter_id, "mayan-solana-swift-v1");
        assert_eq!(builder_addr_of(&att), builder_addr());
        adj.verify(&att).unwrap();
    }

    #[tokio::test]
    async fn lifi_attestation() {
        let adj = CanonicalAdjudicator;
        let signer = make_signer();
        let reg = registry_with_known_adapter("lifi-meta-v2");
        let fill = outcome("lifi", 42161, 1.20);

        let att = adj.attest(&fill, &reg, &signer, ZERO_HASH).await.unwrap();
        assert_eq!(att.adapter_id, "lifi-meta-v2");
        assert_eq!(builder_addr_of(&att), builder_addr());
        adj.verify(&att).unwrap();
    }

    #[tokio::test]
    async fn unknown_adapter_routes_to_ecosystem() {
        let adj = CanonicalAdjudicator;
        let signer = make_signer();
        // Registry has only `across-v3`; "unknown" protocol won't match.
        let reg = registry_with_known_adapter("across-v3");
        let fill = outcome("unknown", 12345, 1.0);

        let att = adj.attest(&fill, &reg, &signer, ZERO_HASH).await.unwrap();
        assert!(att.adapter_id.starts_with("unknown-"));
        assert_eq!(builder_addr_of(&att), ecosystem_addr());
        assert_eq!(reviewer_addrs_of(&att), vec![ecosystem_addr()]);
        adj.verify(&att).unwrap();
    }

    #[tokio::test]
    async fn hash_chain_links() {
        let adj = CanonicalAdjudicator;
        let signer = make_signer();
        let reg = registry_with_known_adapter("across-v3");

        let fill_a = outcome("across", 42161, 0.10);
        let mut fill_b = outcome("across", 42161, 0.20);
        // Ensure B has a distinct fill_id so it's a different record.
        fill_b.intent_id = "intent-test-2".into();
        fill_b.tx_hash = Some("0xdef".into());

        let att_a = adj.attest(&fill_a, &reg, &signer, ZERO_HASH).await.unwrap();
        let next_prev = hash_for_chain(&att_a).unwrap();
        let att_b = adj
            .attest(&fill_b, &reg, &signer, &next_prev)
            .await
            .unwrap();

        assert_eq!(att_b.prev_hash, next_prev);
        assert_ne!(att_b.prev_hash, ZERO_HASH);
        adj.verify(&att_b).unwrap();
    }

    #[tokio::test]
    async fn signature_verification_detects_tampering() {
        let adj = CanonicalAdjudicator;
        let signer = make_signer();
        let reg = registry_with_known_adapter("across-v3");
        let fill = outcome("across", 42161, 1.0);

        let mut att = adj.attest(&fill, &reg, &signer, ZERO_HASH).await.unwrap();
        // Tamper after signing — bump inflow without updating donut_take.
        att.inflow_usd_micro = 1_000_000_000_000;
        let err = adj.verify(&att).unwrap_err();
        let msg = err.to_string();
        // Either math fails (because donut_take wasn't updated) or signature
        // recovery fails. Both are valid tamper-detection outcomes.
        assert!(
            msg.contains("donut_take_usd_micro")
                || msg.contains("recovered")
                || msg.contains("share"),
            "expected tamper-detection error, got: {}",
            msg
        );
    }

    #[tokio::test]
    async fn signature_verification_detects_wrong_signer() {
        let adj = CanonicalAdjudicator;
        let signer_a = make_signer();
        let signer_b = make_signer_b();
        let reg = registry_with_known_adapter("across-v3");
        let fill = outcome("across", 42161, 1.0);

        let mut att = adj
            .attest(&fill, &reg, &signer_a, ZERO_HASH)
            .await
            .unwrap();
        // Reassign spinner_addr to a different key's address while keeping
        // signer_a's signature.
        att.spinner_addr = signer_b.address();
        let err = adj.verify(&att).unwrap_err();
        assert!(
            err.to_string().contains("recovered"),
            "expected signature mismatch, got: {}",
            err
        );
    }

    #[test]
    fn canonical_json_is_key_sorted() {
        let signer = make_signer();
        let mut recipients: BTreeMap<String, RecipientShare> = BTreeMap::new();
        recipients.insert(
            PURPOSE_ADAPTER_BUILDER.into(),
            RecipientShare {
                purpose: PURPOSE_ADAPTER_BUILDER.into(),
                addresses: vec![builder_addr()],
                share_usd_micro: 700_000,
                share_num: DEFAULT_BUILDER_NUM,
                share_den: DEFAULT_SPLIT_DEN,
                is_residual: false,
            },
        );
        recipients.insert(
            PURPOSE_ADAPTER_REVIEWERS.into(),
            RecipientShare {
                purpose: PURPOSE_ADAPTER_REVIEWERS.into(),
                addresses: reviewer_addrs(),
                share_usd_micro: 200_000,
                share_num: DEFAULT_REVIEWERS_NUM,
                share_den: DEFAULT_SPLIT_DEN,
                is_residual: false,
            },
        );
        recipients.insert(
            PURPOSE_ADAPTER_ECOSYSTEM.into(),
            RecipientShare {
                purpose: PURPOSE_ADAPTER_ECOSYSTEM.into(),
                addresses: vec![ecosystem_addr()],
                share_usd_micro: 100_000,
                share_num: DEFAULT_ECOSYSTEM_NUM,
                share_den: DEFAULT_SPLIT_DEN,
                is_residual: true,
            },
        );
        let att = DonutAttestation {
            fill_id: "z".into(),
            spinner_id: "y".into(),
            spinner_addr: signer.address(),
            adapter_id: "x".into(),
            protocol: "w".into(),
            dst_chain: 1,
            fee_usd_micro: 1_000_000,
            actual_profit_usd_micro: 1_000_000,
            inflow_usd_micro: 1_000_000,
            split_num: DEFAULT_INFLOW_REDISTRIBUTION_NUM,
            split_den: DEFAULT_INFLOW_REDISTRIBUTION_DEN,
            donut_take_usd_micro: 1_000_000,
            recipients,
            ts: Utc::now(),
            prev_hash: ZERO_HASH.into(),
            signature_hex: "0xdead".into(),
        };

        let s = canonical_json_for_signing(&att).unwrap();
        // signature_hex must be absent from the signing pre-image.
        assert!(!s.contains("signature_hex"));
        // Top-level keys should be sorted: "actual_profit_usd_micro" comes before "adapter_id".
        let i_actual = s.find("\"actual_profit_usd_micro\"").unwrap();
        let i_adapter = s.find("\"adapter_id\"").unwrap();
        assert!(i_actual < i_adapter, "keys not sorted: {}", s);
    }

    #[test]
    fn adapter_id_mapping() {
        // Mayan Swift — Solana destination
        assert_eq!(
            default_adapter_id("mayan", SOLANA_DST_SENTINEL),
            "mayan-solana-swift-v1"
        );
        assert_eq!(
            default_adapter_id("mayan_swift", SOLANA_DST_WORMHOLE),
            "mayan-solana-swift-v1"
        );
        // Mayan Swift — EVM-only (no Solana on either side)
        assert_eq!(default_adapter_id("mayan", 1), "mayan-evm-swift-v1");
        assert_eq!(default_adapter_id("mayan_swift", 42161), "mayan-evm-swift-v1");
        // LiFi / Across — single adapter per protocol
        assert_eq!(default_adapter_id("lifi", 42161), "lifi-meta-v2");
        assert_eq!(default_adapter_id("across", 42161), "across-v3");
        // deBridge — EVM destination keeps legacy id
        assert_eq!(default_adapter_id("debridge", 1), "debridge-dln-v1");
        assert_eq!(default_adapter_id("debridge_dln", 1), "debridge-dln-v1");
        // deBridge DLN — Solana destination must split out
        assert_eq!(
            default_adapter_id("debridge_dln", SOLANA_DST_DEBRIDGE),
            "debridge-dln-solana-v1"
        );
        // Mayan Flash — must split out from Swift (different Anchor program)
        assert_eq!(
            default_adapter_id("mayan_flash", SOLANA_DST_WORMHOLE),
            "mayan-flash-solana-v1"
        );
        assert_eq!(default_adapter_id("mayan_flash", 1), "mayan-flash-evm-v1");
        // Wormhole NTT — Solana program, must NOT fall through to unknown.
        assert_eq!(
            default_adapter_id("wormhole_ntt", SOLANA_DST_WORMHOLE),
            "wormhole-ntt-solana-v1"
        );
        assert_eq!(
            default_adapter_id("wormhole", SOLANA_DST_WORMHOLE),
            "wormhole-ntt-solana-v1"
        );
    }

    #[test]
    fn adapter_id_routes_solana_source_mayan_to_solana_builder() {
        let rec = OutcomeRecord {
            ts: Utc::now(),
            intent_id: "mayan-redeem-1".into(),
            protocol: "mayan_swift".into(),
            src_chain: SOLANA_DST_WORMHOLE, // 1_399_811_149 — Solana source
            dst_chain: 1,                   // Ethereum destination
            decision: "executed".into(),
            tx_hash: Some("0xabc".into()),
            predicted_gas: None,
            gas_used: Some(150_000),
            effective_gas_price_wei: None,
            predicted_profit_usd: Some(0.5),
            actual_profit_usd: Some(0.5),
            skip_reason: None,
            error: None,
            solver_id: Some("00000000".into()),
            claim_tx_hash: None,
            claim_fee_usd: None,
            fee_usd: None,
        };
        assert_eq!(adapter_id_for_outcome(&rec), "mayan-solana-swift-v1");
    }

    #[tokio::test]
    async fn wormhole_ntt_attestation_routes_to_solana_builder() {
        let adj = CanonicalAdjudicator;
        let signer = make_signer();
        let reg = registry_with_known_adapter("wormhole-ntt-solana-v1");
        let fill = OutcomeRecord {
            ts: Utc::now(),
            intent_id: "ntt-1".into(),
            protocol: "wormhole_ntt".into(),
            src_chain: 8453,                // Base
            dst_chain: SOLANA_DST_WORMHOLE, // Solana
            decision: "executed".into(),
            tx_hash: Some("sig-base58".into()),
            predicted_gas: None,
            gas_used: Some(250_000),
            effective_gas_price_wei: None,
            predicted_profit_usd: Some(0.30),
            actual_profit_usd: Some(0.30),
            skip_reason: None,
            error: None,
            solver_id: Some("00000000".into()),
            claim_tx_hash: None,
            claim_fee_usd: None,
            fee_usd: None,
        };
        let att = adj.attest(&fill, &reg, &signer, ZERO_HASH).await.unwrap();
        assert_eq!(att.adapter_id, "wormhole-ntt-solana-v1");
        assert_eq!(builder_addr_of(&att), builder_addr());
        adj.verify(&att).unwrap();
    }

    #[tokio::test]
    async fn mayan_flash_split_from_swift() {
        let adj = CanonicalAdjudicator;
        let signer = make_signer();
        let reg = registry_with_known_adapter("mayan-flash-solana-v1");
        let fill = outcome("mayan_flash", SOLANA_DST_WORMHOLE, 1.0);
        let att = adj.attest(&fill, &reg, &signer, ZERO_HASH).await.unwrap();
        assert_eq!(att.adapter_id, "mayan-flash-solana-v1");
        assert_eq!(builder_addr_of(&att), builder_addr());
        adj.verify(&att).unwrap();
    }

    #[tokio::test]
    async fn debridge_dln_solana_split_from_evm() {
        let adj = CanonicalAdjudicator;
        let signer = make_signer();
        let reg = registry_with_known_adapter("debridge-dln-solana-v1");
        let fill = outcome("debridge_dln", SOLANA_DST_DEBRIDGE, 0.75);
        let att = adj.attest(&fill, &reg, &signer, ZERO_HASH).await.unwrap();
        assert_eq!(att.adapter_id, "debridge-dln-solana-v1");
        assert_eq!(builder_addr_of(&att), builder_addr());
        adj.verify(&att).unwrap();
    }

    #[tokio::test]
    async fn verify_rejects_high_s_signature() {
        let adj = CanonicalAdjudicator;
        let signer = make_signer();
        let reg = registry_with_known_adapter("across-v3");
        let fill = outcome("across", 42161, 1.0);

        let mut att = adj.attest(&fill, &reg, &signer, ZERO_HASH).await.unwrap();
        // The signer produces a low-s signature; flip to high-s by
        // replacing s with n - s.
        let sig_hex = att
            .signature_hex
            .strip_prefix("0x")
            .unwrap_or(&att.signature_hex);
        let mut sig_bytes = hex::decode(sig_hex).unwrap();
        assert_eq!(sig_bytes.len(), 65);

        // secp256k1 group order n (big-endian).
        const N_BE: [u8; 32] = [
            0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
            0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFE,
            0xBA, 0xAE, 0xDC, 0xE6, 0xAF, 0x48, 0xA0, 0x3B,
            0xBF, 0xD2, 0x5E, 0x8C, 0xD0, 0x36, 0x41, 0x41,
        ];

        // Compute new_s = n - s using big-endian byte subtraction.
        let mut new_s = [0u8; 32];
        let mut borrow: i16 = 0;
        for i in (0..32).rev() {
            let a = N_BE[i] as i16;
            let b = sig_bytes[32 + i] as i16 + borrow;
            let diff = a - b;
            if diff < 0 {
                new_s[i] = (diff + 256) as u8;
                borrow = 1;
            } else {
                new_s[i] = diff as u8;
                borrow = 0;
            }
        }
        sig_bytes[32..64].copy_from_slice(&new_s);
        sig_bytes[64] = match sig_bytes[64] {
            27 => 28,
            28 => 27,
            0 => 1,
            1 => 0,
            other => other ^ 1,
        };

        att.signature_hex = format!("0x{}", hex::encode(&sig_bytes));
        let err = adj.verify(&att).expect_err("high-s signature must reject");
        let msg = err.to_string();
        assert!(
            msg.contains("malleability"),
            "expected malleability error, got: {}",
            msg
        );
    }

    #[tokio::test]
    async fn mayan_solana_fixture_routes_to_solana_builder() {
        let adj = CanonicalAdjudicator;
        let signer = make_signer();
        let reg = registry_with_known_adapter("mayan-solana-swift-v1");

        let fill = OutcomeRecord {
            ts: Utc::now(),
            intent_id: "mayan-fixture-1".into(),
            protocol: "mayan_swift".into(),
            src_chain: 1_399_811_149,
            dst_chain: 1,
            decision: "executed".into(),
            tx_hash: Some("0xfeed".into()),
            predicted_gas: None,
            gas_used: Some(120_000),
            effective_gas_price_wei: None,
            predicted_profit_usd: Some(0.42),
            actual_profit_usd: Some(0.42),
            skip_reason: None,
            error: None,
            solver_id: Some("00000000".into()),
            claim_tx_hash: None,
            claim_fee_usd: None,
            fee_usd: None,
        };
        let att = adj.attest(&fill, &reg, &signer, ZERO_HASH).await.unwrap();
        assert_eq!(att.adapter_id, "mayan-solana-swift-v1");
        // 0.42 USD = 420_000 micro inflow, redistributed 100%.
        assert_eq!(att.donut_take_usd_micro, 420_000);
        adj.verify(&att).unwrap();
    }
}
