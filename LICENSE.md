# License — taifoon-solver

This solver runtime is licensed under **Apache 2.0**. See [`LICENSE`](./LICENSE)
for the operative text.

The upstream platform contracts — the on-chain order contract, the
adapter registry, and the reviewer set — live in a separate upstream
repository and are licensed under **TSUL v1.0** (Taifoon Sustainable
Use License).

**Public TSUL FAQ:** https://taifoon.io/legal/tsul  
**License questions:** taifooon@proton.me

---

## Four TSUL rules (plain English)

1. **You may use and fork freely.** The solver core is Apache 2.0 — no
   restrictions.
2. **You may not offer the platform as a competing hosted service**
   without a commercial agreement.
3. **If your deployment routes value through the upstream adapter
   fleet,** the donut redistribution at the platform contract layer
   must remain in place.
4. **Contributions merge under TSUL.** From the merge block onward,
   70% of every redistributed adapter-owner inflow routes to the
   contributor's wallet — perpetually, on-chain, automatic.

---

## Why the split

Apache 2.0 on the solver core makes the project Public-Goods-eligible
and allows anyone to embed, fork, or integrate freely.

TSUL on the upstream platform contracts ensures the donut (the 70 / 20 /
10 redistribution to adapter_builder / adapter_reviewers /
adapter_ecosystem) survives any commercial deployment — the license IS
the enforcement mechanism at the contract layer.

See [`docs/tsul.md`](./docs/tsul.md) for the full posture rationale and
[`docs/donut_flow.md`](./docs/donut_flow.md) for the runtime architecture.
