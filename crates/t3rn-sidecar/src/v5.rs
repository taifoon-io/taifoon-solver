use serde::{Deserialize, Serialize};

use protocol_adapters::FillResult;

/// V5 evidence skeleton arc-box's L5/L6 reviewer can verdict against.
/// We never forge the full MMR proof here — arc-box treats missing
/// stages as "advisory" when `simulated=true`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V5Skeleton {
    pub fill_event_tx_hash: String,
    pub fill_event_decoded: bool,
    pub finality_type: String,
    pub simulated: bool,
}

impl V5Skeleton {
    pub fn from_fill(result: &FillResult, dst_chain_id: u64, dst_kind: &str) -> Self {
        let finality_type = if dst_kind == "solana" {
            "solana_confirmed".to_string()
        } else if matches!(dst_chain_id, 1) {
            "evm_finalized".to_string()
        } else {
            "evm_safe_head".to_string()
        };
        Self {
            fill_event_tx_hash: result.tx_hash.clone(),
            fill_event_decoded: !result.tx_hash.is_empty(),
            finality_type,
            simulated: result.simulated,
        }
    }
}
