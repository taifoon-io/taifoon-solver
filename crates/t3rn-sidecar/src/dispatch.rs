use anyhow::{anyhow, Result};
use chrono::Utc;
use genome_client::Intent;

use protocol_adapters::{FillResult, V5ProofBlob, L1SuperRoot, L2ChainHeader, L5ChainEvent, L6FinalityCommitment};

use crate::state::{SidecarState, StoredIntent};
use crate::v5::V5Skeleton;

/// Arc-side payload field-for-field matched to arc-api's FillRequest.
/// Kept stable so the sidecar contract doesn't drift from arc-api.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct ArcIntentInbound {
    pub protocol_slug: String,
    pub src_chain_id: u64,
    pub dst_chain_id: u64,
    /// "evm" (default) or "solana". When "solana", dst_chain_id is ignored
    /// for protocol routing but kept in the receipt for traceability.
    #[serde(default = "default_dst_kind")]
    pub dst_kind: String,
    pub src_token: String,
    pub dst_token: String,
    pub input_amount: String,
    #[serde(default)]
    pub output_amount: Option<String>,
    pub recipient: String,
    #[serde(default)]
    pub depositor: Option<String>,
    #[serde(default)]
    pub src_tx_hash: Option<String>,
    #[serde(default)]
    pub dry_run: bool,
}

fn default_dst_kind() -> String {
    "evm".to_string()
}

impl ArcIntentInbound {
    pub fn to_intent(&self, sidecar_intent_id: &str) -> Intent {
        Intent {
            id: sidecar_intent_id.to_string(),
            protocol: self.protocol_slug.clone(),
            src_chain: self.src_chain_id,
            dst_chain: self.dst_chain_id,
            src_token: self.src_token.clone(),
            dst_token: self.dst_token.clone(),
            amount: self.input_amount.clone(),
            depositor: self.depositor.clone().unwrap_or_else(|| self.recipient.clone()),
            recipient: self.recipient.clone(),
            tx_hash: self.src_tx_hash.clone().unwrap_or_default(),
            detected_at: Utc::now().timestamp() as u64,
            output_amount: self.output_amount.clone(),
            deposit_id: None,
            maker_order_nonce: None,
            give_amount: None,
            take_amount: None,
            order_id: None,
            dln_give_patch_authority_src: None,
            dln_order_authority_address_dst: None,
            dln_allowed_taker_dst: None,
            ..Default::default()
        }
    }
}

/// Run the adapter for an intent. Honors the two-key gate: if live fills are
/// locked, force dry-run regardless of requested mode.
pub async fn dispatch(
    state: &SidecarState,
    inbound: &ArcIntentInbound,
    sidecar_intent_id: &str,
) -> Result<StoredIntent> {
    let live_unlocked = state.config.live_fills_unlocked();
    let dry_run_effective = !live_unlocked || inbound.dry_run;
    let now = Utc::now();

    let intent = inbound.to_intent(sidecar_intent_id);
    let adapter = state
        .factory
        .get_adapter(&intent)
        .map_err(|e| anyhow!("no adapter for slug {}: {}", inbound.protocol_slug, e))?;

    // Best-effort gas estimate (advisory; not blocking).
    let _ = adapter
        .estimate_gas(&intent, &state.config.spinner_api_url)
        .await
        .ok();

    // V5 proof — sidecar runs adapter-only; produce a stub bundle. arc-box
    // L5/L6 reviewers degrade to advisory when stages are missing.
    let proof_stub = empty_proof_blob(&inbound.src_tx_hash.clone().unwrap_or_default());

    let fill_tx = adapter
        .build_fill_tx(&intent, &proof_stub)
        .await
        .map_err(|e| anyhow!("build_fill_tx failed: {}", e))?;

    let fill_result: FillResult = adapter
        .execute_fill(&intent, fill_tx, dry_run_effective)
        .await
        .map_err(|e| anyhow!("execute_fill failed: {}", e))?;

    let v5 = V5Skeleton::from_fill(&fill_result, inbound.dst_chain_id, &inbound.dst_kind);

    let status = if fill_result.simulated {
        "simulated"
    } else if fill_result.success {
        "confirmed"
    } else {
        "failed"
    }
    .to_string();

    Ok(StoredIntent {
        sidecar_intent_id: sidecar_intent_id.to_string(),
        protocol_slug: inbound.protocol_slug.clone(),
        src_chain_id: inbound.src_chain_id,
        dst_chain_id: inbound.dst_chain_id,
        dst_kind: inbound.dst_kind.clone(),
        src_token: inbound.src_token.clone(),
        dst_token: inbound.dst_token.clone(),
        input_amount: inbound.input_amount.clone(),
        output_amount: inbound.output_amount.clone(),
        recipient: inbound.recipient.clone(),
        dry_run_requested: inbound.dry_run,
        dry_run_effective,
        status,
        fill_tx: Some(fill_result.tx_hash.clone()).filter(|s| !s.is_empty()),
        fill_block: Some(fill_result.block_number).filter(|n| *n > 0),
        gas_used: Some(fill_result.gas_used).filter(|n| *n > 0),
        value_routed_usd: None, // arc-api computes USD; sidecar reports raw units.
        v5: Some(v5),
        error: None,
        created_at: now,
        updated_at: Utc::now(),
    })
}

fn empty_proof_blob(src_tx_hash: &str) -> V5ProofBlob {
    V5ProofBlob {
        l1_superroot: L1SuperRoot {
            hash: String::new(),
            timestamp: 0,
            chains_included: vec![],
        },
        l2_chain_header: L2ChainHeader {
            chain_id: 0,
            block_number: 0,
            block_hash: String::new(),
            parent_hash: String::new(),
            state_root: String::new(),
            timestamp: 0,
        },
        l3_superroot_proof: vec![],
        l4_block_proof: vec![],
        l5_chain_event: L5ChainEvent {
            tx_hash: src_tx_hash.to_string(),
            tx_index: 0,
            log_index: None,
            encoded_tx: String::new(),
            encoded_receipt: String::new(),
        },
        l6_finality: L6FinalityCommitment {
            finality_type: "advisory".into(),
            commitment_data: "{}".into(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::SidecarConfig;

    fn cfg(sim: bool, live_ok: bool) -> SidecarConfig {
        SidecarConfig {
            port: 0,
            simulation_mode: sim,
            live_fill_ok: live_ok,
            network: "test".into(),
            spinner_api_url: "http://localhost:0".into(),
        }
    }

    #[test]
    fn two_key_gate_default_locked() {
        let c = cfg(true, false);
        assert!(!c.live_fills_unlocked(), "sim=true → locked");
        let c = cfg(false, false);
        assert!(!c.live_fills_unlocked(), "live_ok=no → locked");
        let c = cfg(true, true);
        assert!(!c.live_fills_unlocked(), "sim=true wins even with live_ok");
        let c = cfg(false, true);
        assert!(c.live_fills_unlocked(), "both flipped → unlocked");
    }

    #[test]
    fn inbound_payload_deserializes_with_defaults() {
        let raw = r#"{
            "protocol_slug": "across",
            "src_chain_id": 10,
            "dst_chain_id": 8453,
            "src_token": "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48",
            "dst_token": "0x833589fcd6edb6e08f4c7c32d4f71b54bda02913",
            "input_amount": "1000000",
            "recipient": "0x000000000000000000000000000000000000dEaD"
        }"#;
        let p: ArcIntentInbound = serde_json::from_str(raw).expect("parses");
        assert_eq!(p.dst_kind, "evm");
        assert!(!p.dry_run);
        assert!(p.output_amount.is_none());
    }

    #[test]
    fn solana_destination_marks_dst_kind() {
        let raw = r#"{
            "protocol_slug": "mayan_swift",
            "src_chain_id": 42161,
            "dst_chain_id": 0,
            "dst_kind": "solana",
            "src_token": "0xaf88d065e77c8cC2239327C5EDb3A432268e5831",
            "dst_token": "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
            "input_amount": "5000000",
            "recipient": "9wFFL7eXYjkB1d3CzfpW7Hjxz7yQz9oo5tKQAezL5Tpe",
            "dry_run": true
        }"#;
        let p: ArcIntentInbound = serde_json::from_str(raw).expect("parses");
        assert_eq!(p.dst_kind, "solana");
        assert!(p.dry_run);
    }
}
