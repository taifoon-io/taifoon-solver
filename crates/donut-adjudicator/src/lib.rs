//! Donut adjudicator — signed attestations for TSUL fee splits.
//!
//! ## What this crate does
//!
//! Every fill produced by a Spinner generates an `OutcomeRecord` (see
//! `executor::outcome_log`). When the realized profit is positive, the TSUL
//! contract reserves **49 bps of `max(0, actual_profit_usd)`** as the "donut"
//! and splits it three ways:
//!
//! * **70 %** → Builder (the developer who shipped the adapter contract)
//! * **20 %** → Reviewer set (open-mamba code-review agents)
//! * **10 %** → Ecosystem treasury
//!
//! On EVM paths the split is enforced by `BuildersRegistry.recordRevenueTouch()`
//! at fill time. But two protocol families cannot be enforced on-chain:
//!
//! 1. **Mayan-Solana Swift** — the protocol pays the solver's EOA directly,
//!    there is no relayer hook we can splice the donut into.
//! 2. **LiFi meta-router** — LiFi is itself an aggregator; sub-attribution
//!    happens inside their off-chain accounting.
//!
//! For those two we produce a **signed off-chain attestation**: a structured,
//! deterministically-serialized record of the split that the Spinner signs
//! with their EVM key. Attestations chain by `prev_hash` so the ledger is
//! tamper-evident.
//!
//! ## The donut math
//!
//! **Critical**: the donut is `max(0, actual_profit_usd) * 0.0049`, NOT
//! `gross_notional * 0.0049`. Taking 49 bps of gross would exceed the profit
//! on every fill and bankrupt the Spinner. Losing fills produce a zero donut.
//!
//! ```text
//! donut_take      = max(0, profit) * 0.0049
//! creator_share   = donut_take * 0.70
//! reviewer_share  = donut_take * 0.20   (split equally between reviewers at payout)
//! ecosystem_share = donut_take * 0.10
//! spinner_keeps   = profit - donut_take
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

/// 49 bps — TSUL rule #4.
pub const DONUT_BPS: f64 = 0.0049;
/// Builder receives 70 % of the donut.
pub const CREATOR_FRACTION: f64 = 0.70;
/// Reviewer set receives 20 % of the donut.
pub const REVIEWER_FRACTION: f64 = 0.20;
/// Ecosystem treasury receives 10 % of the donut.
pub const ECOSYSTEM_FRACTION: f64 = 0.10;

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

/// Maps adapter ids to the on-chain addresses that should receive their share
/// of the donut. The Spinner OS loads this from a config file at boot.
#[derive(Debug, Clone)]
pub struct AdapterRegistry {
    /// `adapter_id` → Builder address (70 % recipient).
    pub builders: HashMap<String, Address>,
    /// `adapter_id` → ordered list of reviewer addresses. The 20 % reviewer
    /// pool is divided equally between them at payout time.
    pub reviewers: HashMap<String, Vec<Address>>,
    /// Single ecosystem treasury address. Receives 10 % of every donut.
    pub ecosystem: Address,
}

impl AdapterRegistry {
    pub fn new(ecosystem: Address) -> Self {
        Self {
            builders: HashMap::new(),
            reviewers: HashMap::new(),
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
}

/// Signed, deterministically-hashed record of a single donut split.
///
/// Every field is `#[serde]`-stable. Canonical JSON for hashing/signing is
/// produced by [`canonical_json_for_signing`] (signature field omitted) and
/// [`canonical_json_with_signature`] (signature field included — used for
/// chaining the next `prev_hash`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DonutAttestation {
    pub fill_id: String,
    pub spinner_id: String,
    pub spinner_addr: Address,
    pub adapter_id: String,
    pub protocol: String,
    pub dst_chain: u64,
    pub actual_profit_usd: f64,
    pub donut_take_usd: f64,
    pub creator_addr: Address,
    pub creator_share_usd: f64,
    pub reviewer_addrs: Vec<Address>,
    /// TOTAL reviewer share — divided equally between `reviewer_addrs` at payout.
    pub reviewer_share_usd: f64,
    pub ecosystem_addr: Address,
    pub ecosystem_share_usd: f64,
    pub spinner_keeps_usd: f64,
    pub ts: DateTime<Utc>,
    /// sha256 of the previous attestation's canonical JSON *with* its
    /// signature. Hex with `0x` prefix. Zero-hash for the first attestation.
    pub prev_hash: String,
    /// EIP-191 personal_sign over `canonical_json_for_signing(self)` as bytes.
    /// 0x-prefixed 65-byte hex (r || s || v).
    pub signature_hex: String,
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
/// Builder credit — is owned by the Solana program author even though the
/// fill itself broadcasts to an EVM destination. Keying the adapter id off
/// only `dst_chain` mis-attributes that flow.
pub fn is_solana_involved(src_chain: u64, dst_chain: u64) -> bool {
    is_solana_dst(src_chain) || is_solana_dst(dst_chain)
}

/// Map `(protocol, dst_chain)` → adapter id used to look up Builder /
/// reviewer addresses in an [`AdapterRegistry`].
///
/// **Prefer [`adapter_id_for_outcome`]** when you have access to the full
/// `OutcomeRecord` — it uses both `src_chain` and `dst_chain` and correctly
/// attributes Solana-source-EVM-destination flows to the Solana Builder.
/// This 2-arg variant is kept for callers that only know the destination.
pub fn default_adapter_id(protocol: &str, dst_chain: u64) -> String {
    // Use dst_chain as src_chain fallback (2-arg legacy mode) — prevents
    // chain 0 (SOLANA_DST_SENTINEL) from triggering Solana routing when
    // the actual source is unknown but destination is EVM.
    adapter_id_resolve(protocol, dst_chain, dst_chain)
}

/// Map `(protocol, src_chain, dst_chain)` from an [`OutcomeRecord`] → adapter id.
///
/// The mapping splits each protocol-family into one Builder per program:
///
/// | adapter_id                  | Triggered by                                          | Solana program / EVM contract |
/// |-----------------------------|-------------------------------------------------------|-------------------------------|
/// | `mayan-flash-solana-v1`     | `protocol="mayan_flash"` ∧ Solana involved             | Mayan Flash LP program        |
/// | `mayan-flash-evm-v1`        | `protocol="mayan_flash"` ∧ EVM-only                    | Mayan Flash EVM (future)      |
/// | `mayan-solana-swift-v1`     | `protocol="mayan_swift"`/`"mayan"` ∧ Solana involved   | Mayan Swift Solana program    |
/// | `mayan-evm-swift-v1`        | `protocol="mayan_swift"`/`"mayan"` ∧ EVM-only          | Mayan Swift EVM contract      |
/// | `wormhole-ntt-solana-v1`    | `protocol` contains `wormhole`/`ntt` ∧ Solana involved | Wormhole NTT Solana program   |
/// | `debridge-dln-solana-v1`    | `protocol` contains `debridge`/`dln` ∧ Solana involved | DLN Solana destination prog.  |
/// | `debridge-dln-v1`           | `protocol` contains `debridge`/`dln` ∧ EVM-only        | DLN EVM destination contract  |
/// | `across-v3`                 | `protocol="across"`                                    | Across V3 SpokePool           |
/// | `lifi-meta-v2`              | `protocol="lifi"`                                      | LiFi meta-router              |
/// | `unknown-{p}-{dst}`         | anything else                                          | (fail-closed → ecosystem)     |
pub fn adapter_id_for_outcome(record: &OutcomeRecord) -> String {
    adapter_id_resolve(&record.protocol, record.src_chain, record.dst_chain)
}

fn adapter_id_resolve(protocol: &str, src_chain: u64, dst_chain: u64) -> String {
    let p = protocol.to_ascii_lowercase();
    let solana = is_solana_involved(src_chain, dst_chain);

    // Mayan Flash **must** be matched before the generic Mayan branch
    // because it's a separate Anchor program with a separate Builder.
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

/// Pure math — no signing, no I/O. Pulled out so unit tests can verify the
/// invariants independently of the trait.
fn compute_split(actual_profit_usd: f64) -> (f64, f64, f64, f64, f64) {
    let profit = if actual_profit_usd.is_finite() {
        actual_profit_usd
    } else {
        0.0
    };
    let positive = profit.max(0.0);
    let donut = positive * DONUT_BPS;
    let creator = donut * CREATOR_FRACTION;
    let reviewer = donut * REVIEWER_FRACTION;
    let ecosystem = donut * ECOSYSTEM_FRACTION;
    let keeps = profit - donut;
    (donut, creator, reviewer, ecosystem, keeps)
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
        let profit = fill.actual_profit_usd.unwrap_or(0.0);
        let (donut, creator, reviewer, ecosystem, keeps) = compute_split(profit);

        let adapter_id = adapter_id_for_outcome(fill);
        let ecosystem_addr = registry.ecosystem;

        // Fail-closed: unknown adapter → route the creator + reviewer cuts to
        // the ecosystem treasury. Spinners running unregistered adapters do NOT
        // silently pocket the Builder's share.
        let (creator_addr, reviewer_addrs) = match registry.builders.get(&adapter_id) {
            Some(addr) => {
                let reviewers = registry
                    .reviewers
                    .get(&adapter_id)
                    .cloned()
                    .unwrap_or_default();
                (*addr, reviewers)
            }
            None => {
                warn!(
                    adapter_id = %adapter_id,
                    "unknown adapter — routing builder + reviewer shares to ecosystem"
                );
                (ecosystem_addr, vec![ecosystem_addr])
            }
        };

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
            actual_profit_usd: profit,
            donut_take_usd: donut,
            creator_addr,
            creator_share_usd: creator,
            reviewer_addrs,
            reviewer_share_usd: reviewer,
            ecosystem_addr,
            ecosystem_share_usd: ecosystem,
            spinner_keeps_usd: keeps,
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
            donut_usd = att.donut_take_usd,
            "attestation signed"
        );

        Ok(att)
    }

    fn verify(&self, att: &DonutAttestation) -> Result<()> {
        // 1. Donut math is internally consistent.
        let positive = att.actual_profit_usd.max(0.0);
        let expected_donut = positive * DONUT_BPS;
        if (att.donut_take_usd - expected_donut).abs() > 1e-9 {
            return Err(anyhow!(
                "donut_take_usd mismatch: got {}, expected {}",
                att.donut_take_usd,
                expected_donut
            ));
        }
        let sum = att.creator_share_usd + att.reviewer_share_usd + att.ecosystem_share_usd;
        if (sum - att.donut_take_usd).abs() > 1e-9 {
            return Err(anyhow!(
                "share sum mismatch: {} != donut_take {}",
                sum,
                att.donut_take_usd
            ));
        }
        let expected_keeps = att.actual_profit_usd - att.donut_take_usd;
        if (att.spinner_keeps_usd - expected_keeps).abs() > 1e-9 {
            return Err(anyhow!(
                "spinner_keeps_usd mismatch: {} != {}",
                att.spinner_keeps_usd,
                expected_keeps
            ));
        }

        // 2. Signature recovers to `spinner_addr`.
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
fn derive_fill_id(rec: &OutcomeRecord) -> String {
    match rec.tx_hash.as_deref() {
        Some(h) if !h.is_empty() => format!("{}:{}", rec.intent_id, h),
        _ => format!("{}:{}", rec.intent_id, rec.ts.timestamp_nanos_opt().unwrap_or(0)),
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
        }
    }

    #[tokio::test]
    async fn losing_fill_emits_zero_donut() {
        let adj = CanonicalAdjudicator;
        let signer = make_signer();
        let reg = registry_with_known_adapter("across-v3");
        let fill = outcome("across", 42161, -1.0);

        let att = adj.attest(&fill, &reg, &signer, ZERO_HASH).await.unwrap();
        assert_eq!(att.donut_take_usd, 0.0);
        assert_eq!(att.creator_share_usd, 0.0);
        assert_eq!(att.reviewer_share_usd, 0.0);
        assert_eq!(att.ecosystem_share_usd, 0.0);
        assert!((att.spinner_keeps_usd - (-1.0)).abs() < 1e-12);
        adj.verify(&att).unwrap();
    }

    #[tokio::test]
    async fn winning_fill_math_sums_exactly() {
        let adj = CanonicalAdjudicator;
        let signer = make_signer();
        let reg = registry_with_known_adapter("across-v3");
        let fill = outcome("across", 42161, 0.40);

        let att = adj.attest(&fill, &reg, &signer, ZERO_HASH).await.unwrap();
        // 0.40 * 0.0049 = 0.00196
        assert!((att.donut_take_usd - 0.00196).abs() < 1e-12);
        let sum = att.creator_share_usd + att.reviewer_share_usd + att.ecosystem_share_usd;
        assert!((sum - att.donut_take_usd).abs() < 1e-9);
        // 70/20/10 split
        assert!((att.creator_share_usd - 0.00196 * 0.70).abs() < 1e-12);
        assert!((att.reviewer_share_usd - 0.00196 * 0.20).abs() < 1e-12);
        assert!((att.ecosystem_share_usd - 0.00196 * 0.10).abs() < 1e-12);
        // keeps = profit - donut
        assert!((att.spinner_keeps_usd - (0.40 - 0.00196)).abs() < 1e-12);
        adj.verify(&att).unwrap();
    }

    #[tokio::test]
    async fn mayan_solana_attestation() {
        let adj = CanonicalAdjudicator;
        let signer = make_signer();
        let reg = registry_with_known_adapter("mayan-solana-swift-v1");
        // dst_chain == 0 is the Solana sentinel per spec.
        let fill = outcome("mayan", SOLANA_DST_SENTINEL, 2.50);

        let att = adj.attest(&fill, &reg, &signer, ZERO_HASH).await.unwrap();
        assert_eq!(att.adapter_id, "mayan-solana-swift-v1");
        assert_eq!(att.creator_addr, builder_addr());
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
        assert_eq!(att.creator_addr, builder_addr());
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
        assert_eq!(att.creator_addr, ecosystem_addr());
        assert_eq!(att.reviewer_addrs, vec![ecosystem_addr()]);
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
        // Tamper after signing.
        att.actual_profit_usd = 1_000_000.0;
        let err = adj.verify(&att).unwrap_err();
        let msg = err.to_string();
        // Either math fails (because donut_take wasn't updated) or signature
        // recovery fails. Both are valid tamper-detection outcomes.
        assert!(
            msg.contains("donut_take_usd") || msg.contains("recovered") || msg.contains("share"),
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
        let att = DonutAttestation {
            fill_id: "z".into(),
            spinner_id: "y".into(),
            spinner_addr: signer.address(),
            adapter_id: "x".into(),
            protocol: "w".into(),
            dst_chain: 1,
            actual_profit_usd: 1.0,
            donut_take_usd: 0.0049,
            creator_addr: builder_addr(),
            creator_share_usd: 0.00343,
            reviewer_addrs: reviewer_addrs(),
            reviewer_share_usd: 0.00098,
            ecosystem_addr: ecosystem_addr(),
            ecosystem_share_usd: 0.00049,
            spinner_keeps_usd: 0.9951,
            ts: Utc::now(),
            prev_hash: ZERO_HASH.into(),
            signature_hex: "0xdead".into(),
        };

        let s = canonical_json_for_signing(&att).unwrap();
        // signature_hex must be absent from the signing pre-image.
        assert!(!s.contains("signature_hex"));
        // Top-level keys should be sorted: "actual_profit_usd" comes before "adapter_id".
        let i_actual = s.find("\"actual_profit_usd\"").unwrap();
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
        assert_eq!(
            default_adapter_id("mayan_flash", 1),
            "mayan-flash-evm-v1"
        );
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

    /// Mayan **Solana-source → EVM-destination** redeem path (`dst_chain` is
    /// EVM, e.g. mainnet=1). The Swift Solana program drives this; the
    /// Builder credit belongs to the Solana program author, not the EVM
    /// contract author. Mirrors the live fixture at
    /// `tests/fixtures/mayan_solana.json` which has
    /// `protocol="mayan_swift", src_chain=1399811149, dst_chain=1`.
    #[test]
    fn adapter_id_routes_solana_source_mayan_to_solana_builder() {
        let rec = OutcomeRecord {
            ts: Utc::now(),
            intent_id: "mayan-redeem-1".into(),
            protocol: "mayan_swift".into(),
            src_chain: SOLANA_DST_WORMHOLE,   // 1_399_811_149 — Solana source
            dst_chain: 1,                      // Ethereum destination
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
        };
        assert_eq!(adapter_id_for_outcome(&rec), "mayan-solana-swift-v1");
    }

    /// Wormhole NTT EVM→Solana fill must produce an attestation that signs
    /// and verifies and lands at the right adapter_id (the audit caught
    /// the previous mapping returning `unknown-wormhole_ntt-…`).
    #[tokio::test]
    async fn wormhole_ntt_attestation_routes_to_solana_builder() {
        let adj = CanonicalAdjudicator;
        let signer = make_signer();
        let reg = registry_with_known_adapter("wormhole-ntt-solana-v1");
        // Matches the in-source fixture in
        // crates/protocol-adapters-solana/src/wormhole_ntt.rs:397-414.
        let fill = OutcomeRecord {
            ts: Utc::now(),
            intent_id: "ntt-1".into(),
            protocol: "wormhole_ntt".into(),
            src_chain: 8453,                       // Base
            dst_chain: SOLANA_DST_WORMHOLE,        // Solana
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
        };
        let att = adj.attest(&fill, &reg, &signer, ZERO_HASH).await.unwrap();
        assert_eq!(att.adapter_id, "wormhole-ntt-solana-v1");
        assert_eq!(att.creator_addr, builder_addr());
        adj.verify(&att).unwrap();
    }

    /// Mayan Flash (separate Anchor program, separate Builder) must NOT
    /// collapse into Mayan Swift's adapter_id.
    #[tokio::test]
    async fn mayan_flash_split_from_swift() {
        let adj = CanonicalAdjudicator;
        let signer = make_signer();
        // Register the Flash adapter only — Swift is intentionally absent.
        let reg = registry_with_known_adapter("mayan-flash-solana-v1");
        let fill = outcome("mayan_flash", SOLANA_DST_WORMHOLE, 1.0);
        let att = adj.attest(&fill, &reg, &signer, ZERO_HASH).await.unwrap();
        assert_eq!(att.adapter_id, "mayan-flash-solana-v1");
        assert_eq!(att.creator_addr, builder_addr());
        adj.verify(&att).unwrap();
    }

    /// deBridge DLN Solana destination (chain_id `100_000_001`) must use
    /// the Solana-specific adapter id so the Solana DLN program author gets
    /// credit instead of the EVM DLN Builder.
    #[tokio::test]
    async fn debridge_dln_solana_split_from_evm() {
        let adj = CanonicalAdjudicator;
        let signer = make_signer();
        let reg = registry_with_known_adapter("debridge-dln-solana-v1");
        let fill = outcome("debridge_dln", SOLANA_DST_DEBRIDGE, 0.75);
        let att = adj.attest(&fill, &reg, &signer, ZERO_HASH).await.unwrap();
        assert_eq!(att.adapter_id, "debridge-dln-solana-v1");
        assert_eq!(att.creator_addr, builder_addr());
        adj.verify(&att).unwrap();
    }

    /// Integration: load the on-disk Mayan-Solana fixture and run it
    /// through the full attest path. This is the hackathon-critical
    /// regression test — any future change to `OutcomeRecord` shape or the
    /// genome-client protocol-string convention must keep this passing.
    #[tokio::test]
    async fn mayan_solana_fixture_routes_to_solana_builder() {
        let adj = CanonicalAdjudicator;
        let signer = make_signer();
        let reg = registry_with_known_adapter("mayan-solana-swift-v1");

        // Hand-construct the OutcomeRecord that the executor would emit
        // for the fixture at tests/fixtures/mayan_solana.json
        // (protocol="mayan_swift", src_chain=1399811149, dst_chain=1).
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
        };
        let att = adj.attest(&fill, &reg, &signer, ZERO_HASH).await.unwrap();
        assert_eq!(att.adapter_id, "mayan-solana-swift-v1");
        // 0.42 * 0.0049 = 0.002058
        assert!((att.donut_take_usd - 0.002058).abs() < 1e-9);
        adj.verify(&att).unwrap();
    }
}
