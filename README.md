# taifoon-solver

> **The first solver-as-an-OS. Fair-code. On-chain donut economics.**
> Solana-native cross-chain fills with cryptographic settlement proofs.

**Live demo:** https://solver.taifoon.dev  
**Public dashboard:** https://taifoon.io/os/dispatch  
**License (canonical):** [LICENSE.md](./LICENSE.md) · Apache 2.0 on solver core · TSUL on platform contracts  
**Submitted to:** [Colosseum Solana Frontier 2026](https://colosseum.com/frontier/)

First live fill: [2026-04-28 on Base](https://basescan.org/tx/0x262b9d65d30a973107775d5f94f7ea6a0101593e27b3d6a5869b24edca64180c).

---

## Why this is different

Most cross-chain solvers are built as private, MEV-extractive infrastructure. We didn't build a solver. We built **the operating system that turns solvers into a public good**.

- **9-adapter UniversalOperator fleet** already routes real volume — Across, deBridge, Mayan, Lambda, Hyperlane, LiFi, Squid, CCIP, Stargate. Frontier-day demo plugs a Solana-Mayan flow on stage.
- **V5 MMR proofs anchor every fill** to a SuperRoot. No other Frontier project demos cryptographically provable cross-chain settlement. (See `docs/architecture.md`.)
- **TSUL + on-chain donut** is genuinely novel. The canonical 70/20/10 split on every settled call routes to creator/reviewers/ecosystem — on-chain, automatic, irrevocable.
- **Agentic OS dispatch layer** lets anyone submit a Solana protocol gap and have the fleet pick it up. Submit a job → reviewer agents auto-replay → adapter merges → donut accrues to the contributor. End-to-end, no human in the loop.

---

## License posture

Two licenses, deliberately split:

| Path | License | Why |
|------|---------|-----|
| Solver core (this repo) | **Apache 2.0** | Public Goods. Anyone can fork, embed, integrate. Maximum reach. |
| Upstream platform contracts (separate repo) | **TSUL v1.0** (fair-code) | The donut redistribution must be enforceable. TSUL is the contract. |
| Contributor templates | **Apache 2.0** | The template is unrestricted. Contributions inherit TSUL on merge. |

Public TSUL FAQ: https://taifoon.io/legal/tsul  
License questions: **taifooon@proton.me**

---

## Live demo flow (90 seconds)

`FRONTIER_DEMO.md` has the second-by-second cues. Summary:

```
0:00–0:10   /os/dispatch            "live state of the agent fleet — Brain pulses"
0:10–0:25   /os/submit-job          "anyone can dispatch work to the OS"
0:25–0:40   /builders/bounties      "every route is co-owned under TSUL"
0:40–0:55   /legal/tsul             "four rules, on-chain donut, fair-code"
0:55–1:20   Trigger Solana-Mayan    "real fill, V5 proof anchors settlement"
1:20–1:30   Explorer view            "cryptographically anchored, reproducible"
```

---

## Architecture (one diagram)

```
                        [ INTENT STREAM ]
                                │
                                ▼
                       [ PROFIT CALCULATOR ]
                       fees − gas − liquidity
                                │
                                ▼
              [ TaifoonUniversalOperator(V5Proof, Calldata) ]
                                │
                ┌───────────────┼───────────────┐
                ▼               ▼               ▼
        [ AcrossAdapter ] [ MayanAdapter ] [ LambdaAdapter ]   ... 9 total
                │               │               │
                ▼               ▼               ▼
                       [ ON-CHAIN SETTLEMENT ]
                                │
                                ▼
        [ Spinner receives adapter-owner inflow from upstream registry ]
                signed off-chain attestation → 70 / 20 / 10 split
                adapter_builder / adapter_reviewers / adapter_ecosystem
```

Full architecture: [`docs/donut_flow.md`](./docs/donut_flow.md).

---

## What it does (runtime detail)

Taifoon is a Rust solver that watches intent streams from Across, deBridge, Mayan Swift, and LiFi, evaluates profitability in real time, and fires fills on-chain.

```
Genome SSE stream ──► intent filter ──► profit check ──► lambda executor
                                                              │
                                              Across fillRelay│
                                           DLN fulfillOrder   │
                                       Mayan fulfillSimple    │
                                Mayan Solana sendTransaction  │
                                                              ▼
                                                     outcome SQLite
                                                         │
                                                  REST API + dashboard
```

Four fill paths live today: Across V3 · deBridge DLN · Mayan Swift EVM · Mayan Solana.

---

## Repo layout

```
.
├── README.md                       # this file
├── LICENSE                         # Apache 2.0 operative text
├── LICENSE.md                      # license posture, TSUL pointer
├── FRONTIER_DEMO.md                # 90-second demo flow, second-by-second
├── docs/
│   ├── architecture.md
│   └── tsul.md                     # TSUL posture rationale
├── scripts/
│   └── create-frontier-issues.sh   # gh CLI bulk-creates the 14-issue plan
└── crates/                         # Rust workspace (15 crates)
```

---

## Live agent fleet — `/os/dispatch`

The "Brain" indicator on https://taifoon.io/os/dispatch pulses on every dispatcher tick. Reviews-in-flight chips show which open-mamba reviewer agents are running right now. Recent verdict pills (P/F/I) scroll as agents complete.

---

## Get involved

- Pick an open route under TSUL: https://taifoon.io/builders/bounties
- Submit a new route to the flywheel: https://taifoon.io/os/submit-job
- Run the dispatcher locally: see the public dispatcher API surface and single-ABI-method integration notes published at https://taifoon.io/legal/tsul
- License or bespoke commercial questions: **taifooon@proton.me**

Built by the Taifoon project. Fair-code lineage: https://faircode.io/.
