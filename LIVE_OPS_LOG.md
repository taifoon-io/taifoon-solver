# Live-Ops Log

Append-only log of live-ops verification runs against the local genome stream + estimate adapters.

---

## 2026-04-28 11:57 +02:00 — `[live-ops-consolidated]` run

### Capture window
- Endpoint: `http://127.0.0.1:30081/api/genome/subscribe/sse`
- Duration: 30 s (curl `--max-time 30`, exit 28 = expected timeout)
- Raw lines: 1911 → 637 SSE `data:` events (parse failures: 0)

### Protocols seen
**No intent-bearing events in this window.** Only infrastructure ticks:
| entity.action | count |
|---|---:|
| `block.ingest` | 358 |
| `gas.snapshot` | 273 |
| `superroot.commit` | 6 |

Intent-like fields (`protocol`, `deposit_id`, `src_token`, `input_amount`, `order_id`, `bridge`, …): **0 events**.

### Decode success
- Total events: 637 / 637 parsed (100%)
- Across intents: 0 captured ⇒ `deposit_id=None` check N/A
- Decode-to-`Intent` rate for the window: trivially 0/0 (no intent events)

### Fixture state (no changes — nothing new to capture)
| fixture | shape | required fields present? |
|---|---|---|
| `tests/fixtures/across.json` | flat genome event | ref_hash, ts, src_token (`amount`/`dst_token` etc.) — OK |
| `tests/fixtures/debridge.json` | flat | OK |
| `tests/fixtures/lifi.json` | flat | OK |
| `tests/fixtures/mayan_evm.json` | flat | OK |
| `tests/fixtures/mayan_solana.json` | flat | OK |
| `tests/fixtures/lifi_v2_live.json` | **envelope** (`{captured_at_unix_ms, source, note, events:[…]}`) | inner events carry `src_token`, `input_amount`, `ts`, `order_id` — by design (v2 uses `order_id` not `ref_hash`). OK. |

No stale or missing field-name updates needed.

### Estimate run
- `cargo test -p executor -- estimate` → **0 failed, 0 passed, 5 ignored** (live-RPC gated; all 5 estimate integration tests present and compile-clean: `across`, `debridge`, `lifi`, `mayan_evm`, `mayan_solana`).
- `cargo test -p protocol-adapters` → **4 + 6 = 10 passed, 0 failed, 0 ignored** (lib + integration suite green: factory, routing, full-lifecycle Across/Mayan/deBridge, multi-chain).

### Wallet key
- `security find-generic-password -s mamba-messiah-key -w | wc -c` → **67 bytes** ⇒ **KEY_EXISTS=yes** (66-char hex + newline = 32-byte secp256k1 private key).
- `CHAIN_WIRING_JSON` env var: **unset** ⇒ broadcast still gated.

### Top 3 gaps blocking a real broadcast
1. **No live intent flow in 30 s window.** The genome stream is producing only `block.ingest` / `gas.snapshot` / `superroot.commit` ticks. Without an actual `intent` / `order` entity, end-to-end Across-deposit / LiFi-step / Mayan-fulfill paths cannot be exercised against real on-chain state. Need either (a) a much longer capture window (≥ 10 min), (b) trigger a known-good intent on the producer side, or (c) replay archived intent events through the SSE endpoint.
2. **`CHAIN_WIRING_JSON` unset.** Broadcaster requires per-chain RPC + spoke pool / settlement contract addresses. Without it, even a successful estimate can't progress to `eth_sendRawTransaction`.
3. **LiFi v2 events still lack `bridge`/`tool` selector** (verified in prior `[live-estimate-run]`). When intent events do arrive on `lifi_v2`, the meta-router will soft-skip until producer enriches the SSE payload with the bridge selector.

### Next concrete action
Widen the capture window to **≥ 10 min** with `--max-time 600` and re-run this protocol. If still no intent events, ask the spinner team whether the live producer is currently bridging (or run a synthetic intent injection from the spinner side) so we can validate at least one across/lifi/mayan event end-to-end with `MIN_PROFIT_USD=0.0`.

---
