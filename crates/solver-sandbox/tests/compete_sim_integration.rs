//! Integration tests for CompeteSim — in-process, no network required.

use solver_sandbox::{
    compete_sim::{CompeteSim, SimConfig},
    genome_replay::GenomeEvent,
};

/// Build synthetic intents with `amount_usd` large enough that the 0.15%
/// reward exceeds the $0.30 simulated gas cost (need amount > $200).
/// Each event uses amount = 500_000_000 → $500 USDC at 6 decimals.
fn large_intents(count: usize) -> Vec<GenomeEvent> {
    (0..count)
        .map(|i| GenomeEvent {
            id: format!("0x{:064x}", i + 1),
            kind: "order".to_string(),
            summary: format!("synthetic large fill #{}", i + 1),
            meta: serde_json::json!({
                "src_chain": 1,
                "dst_chain": 8453,
                "amount": "500000000", // $500 USDC (6 decimals) → reward $0.75 > gas $0.30
                "src_token": "0x0000000000000000000000000000000000000000",
                "dst_token": "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913",
                "max_reward": "501000000",
                "recipient": "0x0000000000000000000000000000000000000001",
            }),
            ts: 1700000000 + (i as u64 * 30),
        })
        .collect()
}

// ── Test 1: 3 solvers, 20 synthetic intents ───────────────────────────────────

#[tokio::test]
async fn compete_sim_3_solvers_20_intents() {
    // Use large intents so reward (0.15% of $500 = $0.75) > gas cost ($0.30).
    let events = large_intents(20);

    let config = SimConfig {
        solver_count: 3,
        budget_per_solver_usd: 1000.0,
        well_seed_usd: 5000.0,
        well_chains: vec![8453, 42161, 10],
        duration_secs: 0, // run until events exhausted
        speed_multiplier: 1000.0, // fast
    };

    let sim = CompeteSim::new(config, events);
    let lb = sim.run().await;

    // Must produce 3 entries.
    assert_eq!(lb.solvers.len(), 3, "leaderboard must have 3 solver entries");

    // At least one fill must have happened — all intents are $1 each,
    // every solver starts with $1000, so fills are always affordable.
    assert!(lb.total_fills > 0, "expected total_fills > 0, got {}", lb.total_fills);

    // Well drawdown must be non-negative (can be 0 if solvers used own funds only).
    assert!(
        lb.well_drawdown_usd >= 0.0,
        "well_drawdown_usd must be >= 0, got {}",
        lb.well_drawdown_usd
    );

    // Solvers must be sorted by net_usd descending.
    for pair in lb.solvers.windows(2) {
        assert!(
            pair[0].net_usd >= pair[1].net_usd,
            "solvers must be sorted by net_usd desc: {} < {}",
            pair[0].net_usd,
            pair[1].net_usd
        );
    }

    println!(
        "[compete_sim_3_solvers_20_intents] fills={} drawdown=${:.2}",
        lb.total_fills, lb.well_drawdown_usd
    );
    for s in &lb.solvers {
        println!(
            "  {} fills={} gross=${:.2} gas=${:.2} net=${:.2} lwc=${:.2}",
            s.solver_id, s.fills, s.gross_usd, s.gas_cost_usd, s.net_usd, s.lwc_draws_usd
        );
    }
}

// ── Test 2: well runs dry — budget 0, small well seed, many intents ───────────

#[tokio::test]
async fn compete_sim_well_runs_dry() {
    // 50 intents each requiring $500. Two solvers with $0 own budget — they
    // must draw from the well. Seed the well with only $600 (≈ 1 fill possible
    // before the pool is exhausted). Remaining 49 intents will be skipped.
    let events = large_intents(50);

    let config = SimConfig {
        solver_count: 2,
        // Zero own budget — solvers can ONLY draw from the well.
        budget_per_solver_usd: 0.0,
        well_seed_usd: 600.0, // $600 ≈ 1 fill of $500 then dry
        well_chains: vec![8453], // single chain for determinism
        duration_secs: 0,
        speed_multiplier: 1000.0,
    };

    let sim = CompeteSim::new(config, events);
    let lb = sim.run().await;

    println!(
        "[compete_sim_well_runs_dry] total_intents={} total_fills={}",
        lb.total_intents, lb.total_fills
    );

    // With only $600 in the well and $500 per fill, at most 1 fill can happen.
    // The remaining 49 events should be skipped (insufficient funds).
    assert!(
        lb.total_fills < 50,
        "expected well to run dry: total_fills should be < 50, got {}",
        lb.total_fills
    );

    // Well drawdown must be non-negative.
    assert!(
        lb.well_drawdown_usd >= 0.0,
        "well_drawdown_usd must be non-negative"
    );
}
