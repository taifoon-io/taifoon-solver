# License posture for taifoon-solver

This repo is licensed under **Apache 2.0**. The file `LICENSE` at the
repo root is the operative text.

The upstream platform contracts (the on-chain order contract, the
operator, the adapter fleet, the reviewer registry) live in a separate
upstream repository and are licensed under **TSUL v1.0** — the Taifoon
Sustainable Use License.

## Why the split

Apache 2.0 on the solver core makes the project freely embeddable —
anyone can fork, integrate, or run it. TSUL on the upstream platform
contracts is what guarantees the donut redistribution stays in place
for any commercial deployment that routes value through the adapter
fleet — that's the structural property that keeps the marketplace
non-extractive.

In practice:

- Use this repo (`taifoon-solver`) freely under Apache 2.0.
- If your deployment routes value through the upstream adapter fleet,
  the donut redistribution at the platform contract layer must remain
  in place. That clause lives in the upstream repo, not here.

## What the donut redistribution looks like in this repo

Off-chain, signed attestation per fill — the canonical reference is
[`docs/donut_flow.md`](./donut_flow.md). The math: `donut_take =
max(0, inflow) × split_num / split_den`, then 70 / 20 / 10 across
`adapter_builder`, `adapter_reviewers`, `adapter_ecosystem`. The
canonical constants are served at `GET /api/donut/policy` on the live
solver-api.

## License questions

Email **taifooon@proton.me**.

## Public FAQ

The user-facing TSUL FAQ lives at [taifoon.io/legal/tsul](https://taifoon.io/legal/tsul).
