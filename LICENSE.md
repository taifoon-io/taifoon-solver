# License — taifoon-solver

This solver runtime is licensed under **Apache 2.0**. See [`LICENSE`](./LICENSE) for the operative text.

The platform contracts — `BuildersRegistry`, the adapter fleet, and the open-mamba reviewer set — live in [yawningmonsoon/spinner](https://github.com/yawningmonsoon/spinner) and are licensed under **TSUL v1.0** (Taifoon Sustainable Use License).

**Canonical TSUL:** https://github.com/yawningmonsoon/spinner/blob/master/LICENSE.md  
**Public FAQ:** https://taifoon.io/legal/tsul  
**License questions:** taifooon@proton.me

---

## Four TSUL rules (plain English)

1. **You may use and fork freely.** The solver core is Apache 2.0 — no restrictions.
2. **You may not offer the platform as a competing hosted service** without a commercial agreement.
3. **If your deployment routes value through Taifoon's adapter fleet,** the `BuildersRegistry.recordRevenueTouch()` donut call must remain in place.
4. **Contributions merge under TSUL.** From the merge block onward, 70% of every settled call routes to the contributor's wallet — perpetually, on-chain, automatic.

---

## Why the split

Apache 2.0 on the solver core makes the project Public-Goods-eligible and allows anyone to embed, fork, or integrate freely.  
TSUL on the platform contracts ensures the donut (49 bps split 70/20/10 to creator/reviewers/ecosystem) survives any commercial deployment — the license IS the enforcement mechanism.

See [`docs/tsul.md`](./docs/tsul.md) for the full posture rationale.
