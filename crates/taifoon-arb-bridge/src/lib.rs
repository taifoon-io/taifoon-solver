//! taifoon-arb-bridge — col-p3 consolidation handler.
//!
//! ## Status: **STUB**
//!
//! The col-p3 brief asked us to look in `/Users/mbultra/projects/taifoon-arb/`
//! for an existing **Solana↔Base** bridge adapter and, if present, wire
//! `consolidate(src_chain, dst_chain, amount_usdc)` so idle USDC over $200 is
//! bridged out as a `balance_high` handler on the wallet-manager.
//!
//! What `taifoon-arb` actually contains (verified 2026-04-28):
//!
//! - JavaScript / Node bot for **TRN CEX↔DEX arbitrage** (KuCoin, MEXC,
//!   Uniswap V4 on Arbitrum), see `taifoon-arb/README.md`.
//! - No Rust crate, no Solana code, no Base bridge primitive, no
//!   `consolidate`-style API.
//!
//! Per the brief: "If not found: stub that logs intent and returns Ok(())."
//! That is exactly what this crate does. When a real bridge adapter ships,
//! replace [`StubBridge`] with a live implementation of [`ArbBridge`] and
//! the rest of the wiring (handler trait, solver-main hook) stays the same.
//!
//! ## Real vs stub at a glance
//!
//! | Piece                          | Real?                       | Stub? |
//! |--------------------------------|-----------------------------|-------|
//! | `ArbBridge` trait              | yes (shape is correct)      |       |
//! | `BalanceHighHandler` trait     | yes (shape is correct)      |       |
//! | `StubBridge::consolidate`      |                             | yes — logs and returns Ok |
//! | wallet-manager integration     |                             | yes — gated on `WALLET_BALANCE_HIGH_USDC` env var until col-p2 lands |

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// USDC threshold above which idle balance triggers consolidation.
pub const IDLE_USDC_THRESHOLD: f64 = 200.0;

/// A request to bridge `amount_usdc` of USDC from `src_chain` to `dst_chain`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsolidateRequest {
    pub src_chain: u64,
    pub dst_chain: u64,
    pub amount_usdc: f64,
}

/// Bridge adapter contract. The col-p3 brief describes a Solana↔Base lane,
/// so a real implementation will likely sit on top of CCTP or a Mayan/deBridge
/// route. This trait is chain-agnostic so the same surface works for either.
#[async_trait]
pub trait ArbBridge: Send + Sync {
    async fn consolidate(&self, req: ConsolidateRequest) -> Result<()>;
}

/// Wallet-manager-side hook. When idle USDC on a chain crosses
/// [`IDLE_USDC_THRESHOLD`], the wallet-manager fires this with the relevant
/// chain id and current balance.
#[async_trait]
pub trait BalanceHighHandler: Send + Sync {
    async fn on_balance_high(&self, src_chain: u64, dst_chain: u64, balance_usdc: f64)
        -> Result<()>;
}

/// **STUB** implementation. Logs the intent and returns Ok(()).
///
/// Replace with a real implementation once `taifoon-arb` gains a Rust bridge
/// module (or once we wire CCTP / Mayan-Solana directly here).
pub struct StubBridge;

#[async_trait]
impl ArbBridge for StubBridge {
    async fn consolidate(&self, req: ConsolidateRequest) -> Result<()> {
        tracing::info!(
            src_chain = req.src_chain,
            dst_chain = req.dst_chain,
            amount_usdc = req.amount_usdc,
            "[STUB] taifoon-arb-bridge consolidate — no real bridge wired yet"
        );
        Ok(())
    }
}

/// Default handler: thresholds idle USDC at [`IDLE_USDC_THRESHOLD`] and
/// hands the surplus off to whatever [`ArbBridge`] is plugged in.
pub struct ThresholdHandler<B: ArbBridge> {
    pub bridge: B,
    pub threshold_usdc: f64,
}

impl<B: ArbBridge> ThresholdHandler<B> {
    pub fn new(bridge: B) -> Self {
        Self { bridge, threshold_usdc: IDLE_USDC_THRESHOLD }
    }
}

#[async_trait]
impl<B: ArbBridge> BalanceHighHandler for ThresholdHandler<B> {
    async fn on_balance_high(
        &self,
        src_chain: u64,
        dst_chain: u64,
        balance_usdc: f64,
    ) -> Result<()> {
        if balance_usdc <= self.threshold_usdc {
            tracing::debug!(
                balance_usdc,
                threshold = self.threshold_usdc,
                "balance_high fired but below threshold — no consolidate"
            );
            return Ok(());
        }
        let surplus = balance_usdc - self.threshold_usdc;
        self.bridge
            .consolidate(ConsolidateRequest {
                src_chain,
                dst_chain,
                amount_usdc: surplus,
            })
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn stub_consolidate_returns_ok() {
        let s = StubBridge;
        let r = s
            .consolidate(ConsolidateRequest {
                src_chain: 8453,
                dst_chain: 1399811149,
                amount_usdc: 50.0,
            })
            .await;
        assert!(r.is_ok());
    }

    #[tokio::test]
    async fn handler_below_threshold_noop() {
        let h = ThresholdHandler::new(StubBridge);
        h.on_balance_high(8453, 1399811149, 100.0).await.unwrap();
    }

    #[tokio::test]
    async fn handler_above_threshold_consolidates() {
        let h = ThresholdHandler::new(StubBridge);
        h.on_balance_high(8453, 1399811149, 350.0).await.unwrap();
    }
}
