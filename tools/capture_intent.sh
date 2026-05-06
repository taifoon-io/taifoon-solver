#!/usr/bin/env bash
# tools/capture_intent.sh — Phase 1 fixture grabber
#
# Subscribes to the genome SSE stream for a fixed window, looks for events
# where protocol == "mayan_swift" AND is_solana_source == true, and writes the
# first matching event to tests/fixtures/mayan_solana_live.json. Used to seed
# the table-driven mayan_solana decode tests with a fresh on-chain shape.
#
# If the genome host is unreachable or no matching event arrives in the window,
# the script exits non-zero and leaves the existing fixture in place — never
# invents content. The Phase 1 tests treat the live fixture as optional.
#
# Usage:
#   ./tools/capture_intent.sh                        # 60s default
#   CAPTURE_WINDOW_SECS=120 ./tools/capture_intent.sh
#   GENOME_SSE_URL=http://host:port/api/genome/subscribe/sse ./tools/capture_intent.sh
#
# Exit codes:
#   0  — captured a matching event
#   2  — window expired with no matching event (host reachable but quiet)
#   3  — genome host unreachable
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

CAPTURE_WINDOW_SECS="${CAPTURE_WINDOW_SECS:-60}"
GENOME_SSE_URL="${GENOME_SSE_URL:-https://api.taifoon.dev/api/genome/subscribe/sse}"
OUT="$REPO_ROOT/tests/fixtures/mayan_solana_live.json"
TMP_RAW="$(mktemp -t capture_intent.XXXXXX)"
trap 'rm -f "$TMP_RAW"' EXIT

# Reachability probe: open the SSE stream for ~3s and require at least one byte.
# (The taifoon.dev host does not expose /api/health, so we probe the stream
# itself.) curl returns 28 on the timeout we deliberately impose; success means
# bytes arrived before then.
echo "[capture_intent] Probing genome SSE: $GENOME_SSE_URL"
# `head -c` closing the pipe early SIGPIPEs curl (exit 23/56) — that's normal
# when bytes arrived. Suppress with `|| true` and rely on byte count, not exit.
PROBE_BYTES=$(curl -sN --max-time 3 "$GENOME_SSE_URL" 2>/dev/null | head -c 64 | wc -c | tr -d ' ' || true)
if [ "${PROBE_BYTES:-0}" -lt 1 ]; then
    echo "[capture_intent] Genome SSE unreachable or silent — leaving fixture untouched." >&2
    exit 3
fi

echo "[capture_intent] Subscribing for ${CAPTURE_WINDOW_SECS}s to $GENOME_SSE_URL"
# `curl --no-buffer -N --max-time` keeps the SSE stream open and enforces the
# window without depending on coreutils `timeout` (not present on stock macOS).
# We tee into TMP_RAW so the python filter can stop curl early via SIGPIPE once
# it has captured a matching event. `|| true` swallows curl's exit-28
# (max-time) and exit-23/56 (downstream SIGPIPE) — the matched-event signal is
# the JSON in TMP_RAW, not the pipeline exit code.
curl -fsN --no-buffer --max-time "$CAPTURE_WINDOW_SECS" "$GENOME_SSE_URL" \
    | tee "$TMP_RAW" \
    | python3 - <<'PY' || true
import json, re, sys

# SSE shape: lines beginning with "data:" carry the JSON payload; events are
# separated by blank lines. We scan line-by-line, accumulate the latest data:
# block, and parse + filter on each terminator.
buf = []
def flush():
    global buf
    if not buf:
        return
    raw = "\n".join(buf)
    buf = []
    try:
        ev = json.loads(raw)
    except Exception:
        return
    proto = ev.get("protocol") or ev.get("id")
    is_sol = ev.get("is_solana_source")
    # Wire-shape match: protocol=mayan_swift AND src_chain is Solana
    # (1399811149 is the spinner chain_ids::SOLANA_MAINNET sentinel, but the
    # live stream uses 200 as the Solana chain id, so accept either). The raw
    # SSE does not carry the `is_solana_source` enrichment flag, so we infer
    # it from src_chain. If MayanSolanaIntent::from_intent rejects the captured
    # shape, that's a separate enrichment-pipeline gap (see Phase 1 anomaly).
    if proto == "mayan_swift" and (is_sol is True or ev.get("src_chain") in (200, 1399811149)):
        print(json.dumps(ev, indent=2))
        sys.exit(0)

for line in sys.stdin:
    line = line.rstrip("\n")
    if line.startswith("data:"):
        buf.append(line[5:].lstrip())
    elif line == "":
        flush()
flush()
sys.exit(2)
PY

# python's stdout was captured; re-run in capture mode against TMP_RAW so the
# matched JSON ends up in OUT regardless of stdio routing under `tee`.
python3 - "$TMP_RAW" "$OUT" <<'PY'
import json, sys

raw_path, out_path = sys.argv[1], sys.argv[2]
buf = []
matched = None
with open(raw_path, "r", encoding="utf-8", errors="replace") as fh:
    for line in fh:
        line = line.rstrip("\n")
        if line.startswith("data:"):
            buf.append(line[5:].lstrip())
        elif line == "":
            if buf:
                blob = "\n".join(buf); buf = []
                try:
                    ev = json.loads(blob)
                except Exception:
                    continue
                if (ev.get("protocol") or ev.get("id")) == "mayan_swift" \
                   and ev.get("is_solana_source") is True:
                    matched = ev
                    break
if matched is None and buf:
    try:
        ev = json.loads("\n".join(buf))
        if (ev.get("protocol") or ev.get("id")) == "mayan_swift" \
           and ev.get("is_solana_source") is True:
            matched = ev
    except Exception:
        pass

if matched is None:
    sys.stderr.write("[capture_intent] window expired without a matching event\n")
    sys.exit(2)

with open(out_path, "w", encoding="utf-8") as fh:
    json.dump(matched, fh, indent=2)
    fh.write("\n")
print(f"[capture_intent] wrote {out_path} (intent {matched.get('mayan_order_id') or matched.get('tx_hash')})")
PY
