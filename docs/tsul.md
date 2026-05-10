# License posture for taifoon-solver

This repo is licensed under **Apache 2.0**. The file `LICENSE` at the repo root is the operative text.

The platform contracts (`BuildersRegistry`, the adapter fleet, the open-mamba reviewer set) live in [yawningmonsoon/spinner](https://github.com/yawningmonsoon/spinner) and are licensed under **TSUL v1.0** — the Taifoon Sustainable Use License. Canonical TSUL: https://github.com/yawningmonsoon/spinner/blob/master/LICENSE.md.

## Why split

Apache 2.0 on the solver core makes the project Public-Goods-eligible at Frontier and allows anyone to embed, fork, or integrate the solver freely. TSUL on the platform contracts ensures the on-chain donut (49 bps split 70/20/10 to creator/reviewers/ecosystem) is preserved for any commercial deployment that routes value through the adapter fleet — which is the structural innovation that makes the marketplace non-extractive.

In practice:
- Use this repo (`taifoon-solver`) freely under Apache 2.0.
- If your deployment routes value through Taifoon's adapter fleet, the donut routing in `BuildersRegistry.recordRevenueTouch()` must remain in place. That clause lives at the platform contract layer, not in this repo.

## License questions

Email **taifooon@proton.me**.

## Public FAQ

The user-facing FAQ for TSUL (with worked examples and the four rules) lives at [taifoon.io/legal/tsul](https://taifoon.io/legal/tsul).
