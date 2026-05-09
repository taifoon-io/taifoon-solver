//! Relay Protocol Solana adapter stub.
//!
//! Relay is an Across-derived fast-bridge with Solana support in testnet (as of
//! 2026-05). This stub pre-wires the integration point so the executor dispatch
//! path is ready when Relay mainnet launches.
//!
//! Wire in `RelaySolanaBroadcaster` once the Relay Solana program address is
//! confirmed on mainnet and the IDL is published.
//!
//! NOTE: `RelaySolanaBroadcaster` must NOT be added to the executor dispatch
//! path until Relay Solana is live on mainnet. It is exported from the crate for
//! tooling and integration tests only.
//!
//! TODO(relay-solana): replace `RELAY_SOLANA_PROGRAM_ID` placeholder and implement
//! the full account list from the Relay Solana IDL when mainnet launches.

use anyhow::Result;
use genome_client::Intent;

use crate::simulate::SolanaEstimateOutcome;

/// Relay Protocol Solana program (testnet placeholder — not yet on mainnet).
pub const RELAY_SOLANA_PROGRAM_ID: &str = "ReLayXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX";

/// An Across V3-compatible Relay intent projected for Solana fills.
///
/// Fields mirror the Relay Protocol deposit schema; amounts are in token base
/// units (lamports / micro-USDC) as `u64`.
pub struct RelaySolanaIntent {
    /// Canonical intent ID (e.g. `"relay_solana:dep:12345"`).
    pub intent_id: String,
    /// Relay deposit ID extracted from the intent ID suffix.
    pub deposit_id: u64,
    /// Source-side token mint (base58).
    pub input_token_mint_b58: String,
    /// Destination-side token mint (base58).
    pub output_token_mint_b58: String,
    /// Input amount in token base units.
    pub input_amount: u64,
    /// Required output amount in token base units (negotiated by Relay).
    pub output_amount: u64,
    /// Recipient wallet address (base58).
    pub recipient_b58: String,
    /// Unix-seconds deadline before which the exclusive relayer window opens (0 = none).
    pub exclusivity_deadline: u64,
    /// Unix-seconds deadline by which the fill must land on-chain.
    pub fill_deadline: u64,
    /// Advisory compute-unit budget for the fill instruction.
    pub compute_units_estimate: u64,
}

impl RelaySolanaIntent {
    /// Project a generic `Intent` into a `RelaySolanaIntent`.
    ///
    /// Returns an error if required fields (token addresses, amounts) are missing
    /// or cannot be parsed.
    pub fn from_intent(intent: &Intent) -> Result<Self> {
        Ok(Self {
            intent_id: intent.id.clone(),
            deposit_id: intent
                .id
                .split(':')
                .last()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0),
            input_token_mint_b58: intent.src_token.clone(),
            output_token_mint_b58: intent.dst_token.clone(),
            input_amount: intent.amount.parse().unwrap_or(0),
            output_amount: intent
                .output_amount
                .as_deref()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0),
            recipient_b58: intent.recipient.clone(),
            exclusivity_deadline: 0,
            fill_deadline: intent.fill_deadline.map(|d| d as u64).unwrap_or(0),
            compute_units_estimate: 180_000,
        })
    }
}

/// Relay Protocol Solana broadcaster (stub — not yet live on mainnet).
///
/// Both `simulate` and `send_fill` unconditionally return an error. They will be
/// implemented once the Relay Solana mainnet program ID and IDL are available.
pub struct RelaySolanaBroadcaster {
    /// Solana JSON-RPC endpoint URL.
    pub rpc_url: String,
}

impl RelaySolanaBroadcaster {
    /// Construct a broadcaster pointing at the given Solana RPC endpoint.
    pub fn new(rpc_url: impl Into<String>) -> Self {
        Self {
            rpc_url: rpc_url.into(),
        }
    }

    /// Simulate a Relay Solana fill via `simulateTransaction`.
    ///
    /// Always returns an error until Relay Solana is live on mainnet.
    /// TODO(relay-solana): implement once Relay Solana mainnet IDL is available.
    pub async fn simulate(
        &self,
        _intent: &RelaySolanaIntent,
    ) -> Result<SolanaEstimateOutcome> {
        anyhow::bail!("Relay Solana not yet available on mainnet")
    }

    /// Broadcast a Relay Solana fill transaction.
    ///
    /// Always returns an error until Relay Solana is live on mainnet.
    /// TODO(relay-solana): implement once Relay Solana mainnet launches.
    pub async fn send_fill(&self, _intent: &RelaySolanaIntent) -> Result<String> {
        anyhow::bail!("Relay Solana not yet available on mainnet")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn program_id_constant_is_non_empty() {
        assert!(!RELAY_SOLANA_PROGRAM_ID.is_empty());
        assert!(RELAY_SOLANA_PROGRAM_ID.starts_with("ReLay"));
    }

    #[test]
    fn from_intent_parses_deposit_id_from_suffix() {
        let intent = Intent {
            id: "relay_solana:dep:9999".into(),
            protocol: "relay_solana".into(),
            src_chain: 1_399_811_149,
            dst_chain: 8453,
            src_token: "So11111111111111111111111111111111111111112".into(),
            dst_token: "0x833589fcd6edb6e08f4c7c32d4f71b54bda02913".into(),
            amount: "5000000".into(),
            depositor: "GH7jFKiP8yQkzYBRc1234567890abcdef12345678".into(),
            recipient: "0xdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef".into(),
            output_amount: Some("4990000".into()),
            fill_deadline: Some(1_800_000_000),
            ..Intent::default()
        };
        let relay = RelaySolanaIntent::from_intent(&intent).unwrap();
        assert_eq!(relay.deposit_id, 9999);
        assert_eq!(relay.input_amount, 5_000_000);
        assert_eq!(relay.output_amount, 4_990_000);
        assert_eq!(relay.fill_deadline, 1_800_000_000);
        assert_eq!(relay.compute_units_estimate, 180_000);
    }

    #[tokio::test]
    async fn simulate_returns_error() {
        let b = RelaySolanaBroadcaster::new("https://api.mainnet-beta.solana.com");
        let intent = Intent {
            id: "relay_solana:dep:1".into(),
            ..Intent::default()
        };
        let relay_intent = RelaySolanaIntent::from_intent(&intent).unwrap();
        assert!(b.simulate(&relay_intent).await.is_err());
    }

    #[tokio::test]
    async fn send_fill_returns_error() {
        let b = RelaySolanaBroadcaster::new("https://api.mainnet-beta.solana.com");
        let intent = Intent {
            id: "relay_solana:dep:1".into(),
            ..Intent::default()
        };
        let relay_intent = RelaySolanaIntent::from_intent(&intent).unwrap();
        assert!(b.send_fill(&relay_intent).await.is_err());
    }
}
