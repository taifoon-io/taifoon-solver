# Taifoon Solver — Loopback Agent System

This directory holds the autonomous-delivery loop for the Frontier Hackathon.
Each phase in `FRONTIER_HACKATHON_PLAN.md` runs as a tight loop:

```
coding-agent → review-agent → (PASS) → user broadcasts mainnet gate → close phase
                            ↘ (FAIL) → coding-agent re-prompted with diagnostics
```

## Layout

- `loop_driver.sh` — the orchestrator. Run with a phase number: `./loop_driver.sh 1`
- `agent_prompts/coding_phase{0..6}.md` — system prompts handed to the coding subagent
- `agent_prompts/review_phase{0..6}.md` — system prompts handed to the review subagent
- `gates/phase{0..6}.sh` — automated acceptance gate (no keys required); exit 0 = PASS
- `gates/mainnet_phase{1..4}.sh` — mainnet verification gate runner (requires MB's keys)
- The append-only ledger lives at `../../outcomes/verification_gates.jsonl`

## What the loops do (lift vs. leave)

The session that wrote this scaffold (2026-05-05) **lifted** the heavy implementation work into one shot:
- New `MAX_NOTIONAL_USD` safety belt in lambda controller
- `OutcomeLog::recent()` + `pnl_summary()` helpers
- New `solver-api` endpoints `/api/solver/outcomes` + `/api/solver/pnl`
- `dashboard/components/LivePnL.tsx`
- `run-mainnet.sh` runbook
- Comprehensive plan doc with verification gates

The loops are **left** with the tedious-but-necessary work:
- **Capture and validate live intent fixtures** for each protocol (Across, deBridge, LiFi-via-X, Mayan-Solana). Drive every `Intent::from_genome_event` decode through assertion checks. Report any decode failures with the offending event JSON.
- Run captured intents through `lambda_execute` in DRY_RUN and report **skip-reason histograms** per protocol — flag any reason that fires on >50% of intents (it's almost certainly a regression).
- Cargo clippy / cargo fmt sweep — fix every warning across the workspace.
- Cargo test coverage closure — fill any obvious holes the coverage report flags.
- Mount `<LivePnL/>` on the right dashboard page (one-line edit, but easy to forget).
- Wire the cargo build/test/run-mainnet smoke-test into the gate scripts.
- Stale-doc cleanup (the repo has many `*_PLAN.md` and `SESSION_*.md` files dating back to April; consolidate into `FRONTIER_HACKATHON_PLAN.md` + an `archive/` folder).

## Running a phase loop

```bash
# From the repo root:
./tools/loops/loop_driver.sh 1            # phase 1: Solana broadcast verified
./tools/loops/loop_driver.sh 2            # phase 2: Across mainnet first-fill
# ...
```

The driver:
1. Reads the phase scope from `FRONTIER_HACKATHON_PLAN.md`
2. Invokes the coding agent with `agent_prompts/coding_phase{N}.md` as system prompt
3. After each coding-agent turn, runs `gates/phase{N}.sh` (the acceptance gate)
4. Invokes the review agent with `agent_prompts/review_phase{N}.md` + the gate output
5. If review says PASS, surfaces the mainnet gate command to MB and waits for the verification ledger entry
6. If review says FAIL, re-prompts the coding agent with the review diagnosis
7. Caps loop iterations at `MAX_LOOP_ITERATIONS` (default 6) to prevent infinite churn

The driver is a thin shell wrapper — the actual coding/review agents are spawned via the `claude` CLI (or via the Cowork agent SDK if you prefer programmatic spawning). See `loop_driver.sh` for the integration point.

## Mainnet verification gate ledger

Every successful phase appends one line to `outcomes/verification_gates.jsonl`:

```json
{"phase":1,"tx":"5kZ…aB3","explorer":"https://solscan.io/tx/5kZ…aB3","ts":"2026-05-06T14:22Z","notional_usd":42.10,"realized_usd":1.34}
```

## Kill switch

```bash
pkill -f loop_driver.sh
pkill -SIGTERM taifoon-solver
```
