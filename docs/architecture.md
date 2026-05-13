# Architecture

**This document has been superseded.** The canonical architecture
reference is now [`docs/donut_flow.md`](./donut_flow.md). It covers:

- The Spinner / Solver-module / adapter_builder / adapter_reviewers /
  adapter_ecosystem actor model.
- The signed donut attestation flow (off-chain redistribution of the
  upstream adapter-owner inflow).
- The 70 / 20 / 10 internal split that applies uniformly across every
  adapter a Spinner runs.
- The end-to-end runtime loop: Genome SSE → profit calc →
  `lambda_controller` → protocol-specific fill → outcome log →
  attestation pump.
- The file map (hot files / cold files), test rigs, and per-task entry
  points.

For agent orientation, see [`skills/taifoon-solver/SKILL.md`](../skills/taifoon-solver/SKILL.md).

For the live policy + adapter map served by the running solver-api, see
the public read endpoints:

- `GET /api/donut/policy` — canonical redistribution constants.
- `GET /api/donut/registry` — adapter → builder + reviewer map.
- `GET /api/donut/ledger/:spinner_id` — signed attestation chain.
- `GET /api/donut/ledger/:spinner_id/head` — current chain head.

The dashboard renders these at [`/policy`](https://solver.taifoon.dev/policy).
