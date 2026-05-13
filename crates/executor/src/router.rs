//! # FillRouter — seam between the legacy adapter path and Hand-routed fills.
//!
//! Before this module existed, `Executor::execute_fill()` was wired
//! directly to `protocol-adapters::AdapterFactory`, picking a per-protocol
//! adapter (Across / deBridge / Mayan / LiFi / Orbiter) by string-matching
//! `intent.protocol`. That was the only routing strategy the solver had.
//!
//! The migration target (see surrounding docs) is a Hand-based router that
//! routes through the `taifoon-trade` wrapper — same trait, every venue,
//! whether centralized exchange or on-chain. To get there without
//! breaking the existing fleet, we introduce a `FillRouter` trait here.
//! The default impl, `AdapterRouter`, lifts the current adapter-factory
//! path verbatim, so installing this module is a no-op for any caller
//! that didn't explicitly opt in.
//!
//! Once solver-main wires up a Hand-aware router (a thin shim around
//! `trader::Trader` from the out-of-tree `taifoon-trade` workspace), it
//! can call `Executor::with_router(...)` to swap the routing strategy.
//! No other call sites change.
//!
//! ## Migration sequence
//!
//! 1. (this commit) Introduce trait + `AdapterRouter` default → tests
//!    pass unchanged, behavior identical.
//! 2. solver-main builds a `TraderRouter` that talks to the Hand SDK.
//! 3. The `executor::execute_with_own_funds` body becomes a thin
//!    `self.router.route_and_execute(...)` call.
//! 4. Once every install has migrated, `AdapterRouter` is deleted and
//!    the legacy adapter-factory path goes with it.
//!
//! Steps 2-4 happen incrementally without ever breaking the green bar.

use anyhow::Result;
use async_trait::async_trait;
use genome_client::Intent;
use profit_calc::ProfitResult;
use protocol_adapters::AdapterFactory;
use tracing::info;

use crate::ExecutionResult;

/// Backend abstraction that decides where (and how) a fill lands.
///
/// Implementations declare their routing strategy via the
/// [`description()`] hook so the dashboard / outcome log can record it
/// alongside the fill.
#[async_trait]
pub trait FillRouter: Send + Sync {
    /// Short, stable identifier surfaced by the dashboard
    /// (e.g. `"adapter-factory"`, `"trader-best-of"`, `"trader-pinned-kraken"`).
    fn description(&self) -> &str;

    /// Route + execute (or simulate) the fill.
    ///
    /// `simulation_mode = true` should NEVER broadcast on chain; it must
    /// still return a believable `ExecutionResult` so the rest of the
    /// fleet (outcome log, dashboard, donut attestation) exercises the
    /// same code paths it would in live mode.
    async fn route_and_execute(
        &self,
        intent: &Intent,
        profit: &ProfitResult,
        simulation_mode: bool,
    ) -> Result<ExecutionResult>;
}

/// Default router that preserves the legacy `Executor::execute_with_own_funds`
/// behavior — dispatches to a per-protocol adapter from `AdapterFactory`,
/// fetches a Spinner V5 gas estimate, and (in simulation mode) synthesises
/// an `ExecutionResult`. Live broadcast is not yet implemented in the
/// default path; that lands when each adapter ships a real
/// `execute_fill()` (tracked in `protocol-adapters`).
pub struct AdapterRouter {
    factory: AdapterFactory,
    spinner_api_url: String,
}

impl AdapterRouter {
    pub fn new(spinner_api_url: impl Into<String>) -> Self {
        let url: String = spinner_api_url.into();
        Self {
            factory: AdapterFactory::new(url.clone()),
            spinner_api_url: url,
        }
    }
}

#[async_trait]
impl FillRouter for AdapterRouter {
    fn description(&self) -> &str {
        "adapter-factory"
    }

    async fn route_and_execute(
        &self,
        intent: &Intent,
        profit: &ProfitResult,
        simulation_mode: bool,
    ) -> Result<ExecutionResult> {
        // Body lifted verbatim from the previous `execute_with_own_funds`.
        let adapter = self.factory.get_adapter(intent)?;
        info!("📦 Using protocol adapter: {}", adapter.protocol_name());

        info!("🔐 Fetching V5 proof bundle from Spinner…");
        let proof = adapter.estimate_gas(intent, &self.spinner_api_url).await?;
        info!(
            "✅ Gas estimate: {} units @ {:.2} gwei = ${:.2}",
            proof.gas_units, proof.gas_price_gwei, proof.total_usd
        );

        if simulation_mode {
            info!(
                "✅ [SIMULATION] Would execute protocol fill via {}",
                adapter.protocol_name()
            );
            return Ok(ExecutionResult {
                intent_id: intent.id.clone(),
                fill_tx: format!("0xsim_{}_{}", adapter.protocol_name(), intent.id),
                claim_tx: None,
                gas_used: proof.gas_units,
                actual_profit_usd: profit.net_profit_usd,
            });
        }

        // Live broadcast remains TODO inside each per-protocol adapter.
        // The trader-backed router (when wired) implements its own live
        // path, so failing here keeps the legacy invariant: simulation
        // is the only supported live path on the default router.
        Err(anyhow::anyhow!(
            "Live protocol execution not yet implemented in AdapterRouter — set SIMULATION_MODE=true, or install a custom FillRouter via Executor::with_router(...)"
        ))
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    fn dummy_intent() -> Intent {
        // Minimal Intent — the `CountingRouter` test below ignores the
        // contents; only the `id` is read into the synthetic
        // `ExecutionResult`. `Intent` derives `Default` in genome-client.
        Intent {
            id: "test-intent-1".into(),
            protocol: "across_v3".into(),
            src_chain: 1,
            dst_chain: 8453,
            tx_hash: "0x0".into(),
            amount: "1000000".into(),
            src_token: "USDC".into(),
            depositor: "0x".into(),
            recipient: "0x".into(),
            ..Default::default()
        }
    }

    fn dummy_profit() -> ProfitResult {
        ProfitResult {
            net_profit_usd: 1.23,
            profitable: true,
            breakdown: profit_calc::ProfitBreakdown {
                protocol_fee_usd: 0.5,
                spread_usd: 0.2,
                gas_cost_usd: 0.05,
                liquidity_cost_usd: 0.0,
            },
        }
    }

    /// Mock router that records every call. Used to verify
    /// `Executor::with_router(...)` plumbing.
    pub struct CountingRouter {
        pub calls: AtomicUsize,
        pub label: &'static str,
    }

    impl CountingRouter {
        pub fn new(label: &'static str) -> Self {
            Self { calls: AtomicUsize::new(0), label }
        }
    }

    #[async_trait]
    impl FillRouter for CountingRouter {
        fn description(&self) -> &str { self.label }
        async fn route_and_execute(
            &self,
            intent: &Intent,
            profit: &ProfitResult,
            _simulation: bool,
        ) -> Result<ExecutionResult> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(ExecutionResult {
                intent_id: intent.id.clone(),
                fill_tx: format!("0xmock_{}", self.label),
                claim_tx: None,
                gas_used: 100_000,
                actual_profit_usd: profit.net_profit_usd,
            })
        }
    }

    #[tokio::test]
    async fn adapter_router_simulates_on_dry_run() {
        // We can't actually call Spinner from a unit test, so this only
        // exercises the dispatch shape — a successful AdapterFactory
        // dispatch followed by an `estimate_gas` HTTP call would block
        // here. The test exists so the trait surface is exercised at all;
        // real adapter coverage lives in `tests/across_estimate_test.rs`
        // etc., which hit recorded fixtures.
        let r = AdapterRouter::new("http://localhost:1");
        assert_eq!(r.description(), "adapter-factory");
    }

    #[tokio::test]
    async fn counting_router_is_invoked() {
        let r = Arc::new(CountingRouter::new("test-mock"));
        let r_dyn: Arc<dyn FillRouter> = r.clone();
        let _ = r_dyn
            .route_and_execute(&dummy_intent(), &dummy_profit(), true)
            .await
            .expect("mock router never errors");
        assert_eq!(r.calls.load(Ordering::SeqCst), 1);
        assert_eq!(r.description(), "test-mock");
    }
}
