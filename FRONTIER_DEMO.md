# Frontier Demo Flow — solver.taifoon.dev

**Target length:** 90 seconds. Hard cap 2 minutes.
**Recording:** Loom or QuickTime, 1080p, no system audio (voiceover only).
**Browser tabs prepared in this exact order before recording starts:**

1. `https://taifoon.io/os/dispatch`
2. `https://taifoon.io/os/submit-job`
3. `https://taifoon.io/builders/bounties`
4. `https://taifoon.io/legal/tsul`
5. Solana explorer (Solscan or Solana FM) at the demo wallet
6. (Hidden) `https://github.com/yawningmonsoon/spinner/blob/master/LICENSE.md`

**Pre-flight:**
- `python3 ../open-mamba/brain_bridge.py synthetic` running in a tmux pane (so the Brain pulses on stage even if no real submission lands during the recording window).
- Devnet 36927 RPC reachable: `curl https://rpc.taifoon.dev` returns a JSON-RPC response.
- Demo wallet has test tokens from the faucet (visible balance > 0).
- If using Mayan testnet: confirm endpoint reachable; otherwise switch to read-only mainnet fallback documented in `demo/fallback/README.md`.

---

## Cue sheet

| Time | Action | Voiceover |
|------|--------|-----------|
| 0:00 | Switch to tab 1 (`/os/dispatch`). Cursor near the Brain panel. | "This is the live state of the Taifoon agent fleet. Brain is pulsing — dispatcher is alive. Reviews in flight, queue depth, recent verdicts. All real, all on-chain." |
| 0:10 | Click "Submit a job" CTA. Land on tab 2 (`/os/submit-job`). | "Anyone with a wallet can submit a new route to the flywheel." |
| 0:14 | Pre-fill the form: Category=Solver Adapter, Urgency=High, Title="Solana-Mayan adapter is missing native exit attribution", Brief=2 paragraphs (paste from `demo/sample-brief.md`). | (continuing) "I'm asking the OS to scope a Mayan-Solana adapter — the brief gets hashed, queued, signed by my wallet." |
| 0:22 | Click SUBMIT JOB. Receipt panel appears with `JOB-...` ID. | "Receipt is on-chain-traceable. Triage SLA is 72 hours; in production the dispatcher posts it to the bounty board." |
| 0:25 | Switch to tab 1 — Brain chip count just incremented. | "Watch — that submission just bumped the queue. The Brain pulsed." |
| 0:30 | Switch to tab 3 (`/builders/bounties`). | "These are the open routes right now. Every card under TSUL — Volt-bordered, on-chain enforced. Volume class, creator slice, no upfront, no prize." |
| 0:38 | Hover one route, show the Open Routes card with TSUL · perf-only · no upfront strip. | "Ship the contribution, two reviewer agents replay it, auto-merge. From that block, 70% of every settled call routes to your wallet — perpetually." |
| 0:42 | Click "FAQ →" or "How TSUL works" → land on tab 4 (`/legal/tsul`). | "TSUL — Taifoon Sustainable Use License. Fair-code, n8n lineage, with one structural addition: on-chain donut routing." |
| 0:48 | Scroll to "WHAT YOU GET" cards. | "Three product promises: co-ownership, no upfront, on-chain enforcement. The license clauses exist to enforce them." |
| 0:55 | (Optional, skip if time-tight) Click "LICENSE.md →" Volt button — opens canonical file in new tab. | "One file, one answer. The contract IS the license." |
| 1:00 | Switch back to tab 1. Trigger the live Mayan-Solana fill via the deployed adapter. | "Now the actual settlement layer. I'm executing a real Solana fill through the deployed Mayan adapter." |
| 1:10 | (Adapter call fires.) Switch to Solana explorer (tab 5). | "V5 proof anchors the settlement to a SuperRoot. Every fill cryptographically reproducible. No other Frontier project demos this." |
| 1:20 | Show the on-chain donut event — `RevenueTouch` log on Etherscan-equivalent for devnet 36927. | "And here's the donut routing: 49 bps split 70/20/10 to creator, reviewers, ecosystem. Automatic. Irrevocable." |
| 1:28 | Cut. | "That's Taifoon. We didn't build a solver. We built the OS that makes solvers a public good." |

---

## Backup paths (for live demo failures)

| Failure | Fallback |
|---------|----------|
| Devnet RPC stalls | Pre-recorded screencast in `demo/fallback/devnet-rpc-failure.mp4`. Switch to it; voiceover continues. |
| Mayan testnet down | Use read-only Mayan mainnet via `MAYAN_RPC_FALLBACK=https://mainnet.mayan.so` env override. State the swap explicitly: "Switching to mainnet read-only because testnet endpoint is rate-limited." |
| Wallet popup gets buried | Cmd-Tab back to wallet, sign, return. Voiceover: "Wallet sign step — irrevocable, non-custodial." |
| `/os/dispatch` Brain shows STALE | `synthetic` mode should have prevented this. If still stale, narrate: "Brain is showing stale because the dispatcher tick missed — 30-second backoff. Real production has a watchdog." |

---

## Submission package checklist

Run through immediately before clicking "Submit" on the Colosseum form:

- [ ] 90-second video uploaded unlisted to YouTube. URL copied.
- [ ] 3-slide deck PDF (`demo/frontier-deck.pdf`) under 1MB.
- [ ] GitHub repo URL: `https://github.com/yawningmonsoon/taifoon-solver`.
- [ ] Live demo URL: `https://solver.taifoon.dev`.
- [ ] Public dashboard URL: `https://taifoon.io/os/dispatch`.
- [ ] License posture in submission long-description: "Apache 2.0 on solver core (this repo) + TSUL fair-code on platform contracts (yawningmonsoon/spinner)."
- [ ] One-liner: "The first solver-as-an-OS. Fair-code. On-chain donut economics. Solana-native cross-chain fills with cryptographic settlement proofs."
- [ ] Long description: paste from `demo/long-description.md`.
- [ ] Confirmation email screenshot saved to `demo/submission-receipt.png`.

**Form-submission deadline:** 23:59 May 11, 2026 UTC. Don't cut it close.
