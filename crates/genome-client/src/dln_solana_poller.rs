//! DLN v2 Solana source poller.
//!
//! Watches the DLN Solana program for `OrderCreated` events emitted when a
//! trader opens a cross-chain order on Solana targeting an EVM destination.
//! The solver fills the EVM side using the existing `DlnDestination.fulfillOrder`
//! path — no Solana-side action is required from the solver.
//!
//! Implementation strategy:
//!   1. Call `getSignaturesForAddress` to find recent transactions for the DLN
//!      Solana program.
//!   2. For each new signature, fetch the full transaction via `getTransaction`
//!      and parse `OrderCreated` log lines.
//!   3. Convert each decoded order to an `Intent` with:
//!        - `src_chain`:  1_399_811_149 (informational Solana mainnet chain ID)
//!        - `dst_chain`:  extracted EVM chain ID from the order
//!        - `protocol`:   "dln_solana_source"
//!        - `is_dln_source_solana`: Some(true)
//!
//! NOTE: This is a STUB implementation. Actual log parsing requires knowledge of
//! the DLN Solana program's event discriminator which is not yet published in a
//! stable IDL. The poller compiles, wires cleanly into the solver, and is opt-in
//! via the `ENABLE_DLN_SOLANA_SOURCE` environment variable.
//!
//! TODO(dln-solana-v2): implement log parsing once the DLN Solana mainnet IDL is
//! published.

use anyhow::Result;
use tokio::sync::mpsc;
use tracing::{info, warn};

use crate::Intent;

/// DLN Solana program address (placeholder — replace once mainnet program is confirmed).
pub const DLN_SOLANA_PROGRAM_ID: &str = "dln1xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx";

/// Polls the DLN Solana program for new `OrderCreated` events.
///
/// These are orders where the **source** is Solana and the **destination** is an
/// EVM chain. The solver fills the EVM side using the existing
/// `DlnDestination.fulfillOrder` path.
///
/// Enabled via the `ENABLE_DLN_SOLANA_SOURCE` environment variable (any non-empty
/// value activates the poller). When disabled, no goroutine is spawned and no
/// Solana RPC calls are made.
pub struct DlnSolanaSourcePoller {
    /// Solana JSON-RPC endpoint URL.
    pub rpc_url: String,
    /// DLN Solana program address (base58).
    pub program_id: String,
    /// Seconds between each `getSignaturesForAddress` sweep.
    pub poll_interval_secs: u64,
}

impl Default for DlnSolanaSourcePoller {
    fn default() -> Self {
        Self {
            rpc_url: std::env::var("SOLANA_RPC_URL")
                .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".into()),
            program_id: DLN_SOLANA_PROGRAM_ID.into(),
            poll_interval_secs: 5,
        }
    }
}

impl DlnSolanaSourcePoller {
    /// Construct a poller with a custom RPC URL (program_id and interval use defaults).
    pub fn new(rpc_url: impl Into<String>) -> Self {
        Self {
            rpc_url: rpc_url.into(),
            ..Self::default()
        }
    }

    /// Poll loop — discovers new DLN Solana orders and sends them as `Intent`s to `tx`.
    ///
    /// Runs until the receiver end of `tx` is dropped (i.e. the solver shuts down).
    pub async fn run(self, tx: mpsc::Sender<Intent>) {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .user_agent("Mozilla/5.0 (compatible; taifoon-solver/1.0)")
            .build()
            .unwrap_or_default();
        let mut last_signature: Option<String> = None;

        info!(
            "🌊 DlnSolanaSourcePoller started (rpc={} program={})",
            self.rpc_url,
            &self.program_id[..self.program_id.len().min(20)],
        );

        loop {
            match self
                .fetch_new_orders(&client, last_signature.as_deref())
                .await
            {
                Ok((intents, new_last_sig)) => {
                    if let Some(sig) = new_last_sig {
                        last_signature = Some(sig);
                    }
                    for intent in intents {
                        info!(
                            "📡 DlnSolanaSourcePoller: {} {}→{} {}",
                            intent.id, intent.src_chain, intent.dst_chain, intent.amount
                        );
                        if tx.send(intent).await.is_err() {
                            // Receiver dropped — solver is shutting down.
                            return;
                        }
                    }
                }
                Err(e) => {
                    warn!("DlnSolanaSourcePoller fetch error: {e}");
                }
            }
            tokio::time::sleep(std::time::Duration::from_secs(self.poll_interval_secs)).await;
        }
    }

    /// Fetch newly created DLN orders from the Solana program logs.
    ///
    /// Returns `(intents, Option<newest_signature>)`. The newest signature is
    /// stored by the caller and passed as `after_sig` on the next poll to avoid
    /// re-processing already-seen transactions.
    ///
    /// # Stub note
    /// This implementation always returns an empty list. The real implementation
    /// will call `getSignaturesForAddress` followed by `getTransaction` for each
    /// new signature and parse the `OrderCreated` discriminator from the program
    /// logs.
    ///
    /// TODO(dln-solana-v2): parse DLN Solana `OrderCreated` events from program logs
    /// once the DLN Solana mainnet IDL is published.
    async fn fetch_new_orders(
        &self,
        client: &reqwest::Client,
        _after_sig: Option<&str>,
    ) -> Result<(Vec<Intent>, Option<String>)> {
        // Suppress unused warning — will be used by the real implementation.
        let _ = client;
        // Stub: returns empty. No RPC calls are made.
        Ok((vec![], None))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_poller_fields() {
        let p = DlnSolanaSourcePoller {
            rpc_url: "https://api.mainnet-beta.solana.com".into(),
            program_id: DLN_SOLANA_PROGRAM_ID.into(),
            poll_interval_secs: 5,
        };
        assert_eq!(p.program_id, DLN_SOLANA_PROGRAM_ID);
        assert_eq!(p.poll_interval_secs, 5);
    }

    #[test]
    fn new_overrides_rpc_url() {
        let p = DlnSolanaSourcePoller::new("https://my-custom-rpc.example.com");
        assert_eq!(p.rpc_url, "https://my-custom-rpc.example.com");
        assert_eq!(p.program_id, DLN_SOLANA_PROGRAM_ID);
    }
}
