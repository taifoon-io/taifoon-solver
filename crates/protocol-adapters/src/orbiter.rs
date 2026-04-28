//! Orbiter Finance Adapter
//!
//! Orbiter is a direct-transfer bridge: the solver sends `output_amount` of the
//! destination token directly to the recipient on the destination chain.
//!
//! ## Fill Mechanism
//! - ERC-20 token: call `transfer(recipient, output_amount)` on dst_token contract
//! - Native ETH: send ETH value directly to recipient address
//!
//! ## Profit Model
//! Profit = input_amount * price_in - output_amount * price_out - gas
//! (spread captured between what user deposited vs. what solver pays out)
//!
//! ## Double-Spend Protection
//! None at the protocol level — solver must track fills internally to avoid
//! sending duplicate transfers for the same intent.

use super::*;
use alloy::primitives::{Address, U256};
use alloy::sol;
use alloy::sol_types::SolCall;

// ── ERC-20 ABI (minimal, just transfer) ──────────────────────────────────────

sol! {
    interface IERC20 {
        function transfer(address to, uint256 amount) external returns (bool);
    }
}

// ── OrbiterAdapter ────────────────────────────────────────────────────────────

pub struct OrbiterAdapter {
    _spinner: SpinnerClient,
}

impl OrbiterAdapter {
    pub fn new(spinner_client: SpinnerClient) -> Self {
        Self { _spinner: spinner_client }
    }

    fn is_native_token(token: &str) -> bool {
        let t = token.to_lowercase();
        t == "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"
            || t == "0x0000000000000000000000000000000000000000"
            || t == "eth"
            || t == "native"
    }
}

#[async_trait::async_trait]
impl ProtocolAdapter for OrbiterAdapter {
    fn protocol_name(&self) -> &str {
        "orbiter_finance"
    }

    fn can_handle(&self, intent: &Intent) -> bool {
        intent.protocol.to_lowercase().contains("orbiter")
    }

    async fn estimate_gas(&self, _intent: &Intent, _spinner_api: &str) -> Result<GasEstimate> {
        // ERC-20 transfer: ~65k gas; ETH send: ~21k gas.
        // We return a conservative estimate; profit_calc uses its own gas model.
        Ok(GasEstimate {
            gas_units: 65_000,
            gas_price_gwei: 1.0,
            total_eth: 0.000065,
            total_usd: 0.15,
            destination_chain: 0,
        })
    }

    async fn build_fill_tx(&self, intent: &Intent, _proof: &V5ProofBlob) -> Result<FillTransaction> {
        let recipient: Address = intent.recipient.parse()
            .map_err(|_| anyhow!("invalid recipient address: {}", intent.recipient))?;

        let output_amount_str = intent.output_amount.as_deref().unwrap_or("0");
        let output_amount: U256 = output_amount_str.parse()
            .map_err(|_| anyhow!("invalid output_amount: {}", output_amount_str))?;

        if Self::is_native_token(&intent.dst_token) {
            // Native ETH transfer — send value directly to recipient
            Ok(FillTransaction {
                to: intent.recipient.clone(),
                data: "0x".to_string(),
                value: Some(format!("0x{:x}", output_amount)),
                chain_id: intent.dst_chain,
                estimated_gas: Some(21_000),
            })
        } else {
            // ERC-20 transfer(recipient, amount) on dst_token contract
            let call = IERC20::transferCall {
                to: recipient,
                amount: output_amount,
            };
            let calldata = call.abi_encode();

            Ok(FillTransaction {
                to: intent.dst_token.clone(),
                data: format!("0x{}", hex::encode(calldata)),
                value: None,
                chain_id: intent.dst_chain,
                estimated_gas: Some(65_000),
            })
        }
    }

    async fn execute_fill(
        &self,
        intent: &Intent,
        fill_tx: FillTransaction,
        dry_run: bool,
    ) -> Result<FillResult> {
        if dry_run {
            tracing::info!(
                "🔵 [DRY-RUN] orbiter fill: {} → {} on chain {} ({})",
                intent.id, intent.recipient, intent.dst_chain,
                if fill_tx.value.is_some() { "native ETH" } else { "ERC-20" }
            );
            return Ok(FillResult {
                tx_hash: format!("0xdry_{}", &intent.id[..intent.id.len().min(16)]),
                gas_used: fill_tx.estimated_gas.unwrap_or(65_000),
                block_number: 0,
                success: true,
                simulated: true,
            });
        }

        tracing::warn!(
            "⚠️  orbiter live execute not yet implemented for intent {}",
            intent.id
        );
        Err(anyhow!("orbiter live broadcast not implemented — set SIMULATION_MODE=true"))
    }

    async fn claim_funds(&self, _intent: &Intent, _fill_result: &FillResult) -> Result<ClaimResult> {
        // Orbiter has no on-chain claim step — profit is realized at fill time
        Ok(ClaimResult {
            tx_hash: "0x0".to_string(),
            claimed_amount: "0".to_string(),
            claimed_token: "N/A".to_string(),
        })
    }
}
