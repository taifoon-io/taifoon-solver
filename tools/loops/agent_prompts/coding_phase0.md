# Coding Agent — Phase 0 (Bench: build green, tests green, loops armed)

You are the coding subagent for Phase 0 of the Frontier Hackathon delivery plan
(`FRONTIER_HACKATHON_PLAN.md` §4). Your job is to make the workspace bench-green
and verify the loop scaffolding works.

## Acceptance gate
`./tools/loops/gates/phase0.sh` must exit 0:
- `cargo build --workspace --release`
- `cargo test --workspace`
- `tools/loops/{agent_prompts,gates}` directories exist; `loop_driver.sh` is executable
- `outcomes/verification_gates.jsonl` exists (empty is fine)

## What to do
1. Run `cargo build --workspace --release`. Fix any compilation errors. The most
   likely culprits are the recent additions to `crates/executor/src/outcome_log.rs`
   (new `recent()` and `pnl_summary()` methods) and `crates/solver-api/src/lib.rs`
   (new `OnceLock` field, new `outcomes_handler` / `pnl_handler` routes). If
   those fail, the issue is almost certainly a missing import or a feature flag.
2. Run `cargo test --workspace`. Fix any test failures the **same session** introduced;
   pre-existing failures (e.g. `genome-client::test_parse_genome_event` if not yet
   fixed) should be triaged separately.
3. Verify `./tools/loops/gates/phase0.sh` exits 0.

## What NOT to do
- Do not modify the schema of `solver_outcomes` (it's load-bearing for the dashboard).
- Do not touch `lambda_controller.rs` broadcast branches — Phase 0 is build-quality only.
- Do not introduce new dependencies beyond what's already in `Cargo.toml`.

## Report when done
List the files you changed, the gate output (last 20 lines), and any tests you skipped.
