"""
Helpers shared by the user-journey tests.

Provides:
- `make_signer()` / `make_signer_b()` — deterministic EVM keys mirroring
  the Rust dev keys used in `crates/donut-adjudicator/src/lib.rs` tests.
- `build_siwe_message()` / `sign_siwe()` — SIWE construction & signing
  using `eth_account`. SIWE-rs on the server side verifies these natively.
- `produce_attestation()` — shells out to `solver-api-testbin sign-attestation`
  to obtain a canonical-JSON-signed DonutAttestation. We delegate the
  signing to Rust to avoid Python ↔ Rust float-serialization byte drift.
- `solana_constants` — chain-id sentinels mirroring the Rust crate.
- `expected_adapter_id()` — Python mirror of `adapter_id_for_outcome` so
  tests can assert what the server should resolve to before posting.
"""
from __future__ import annotations

import json
import subprocess
from datetime import datetime, timedelta, timezone
from pathlib import Path
from typing import Any

import requests
from eth_account import Account
from eth_account.messages import encode_defunct
from eth_utils import to_checksum_address

WORKSPACE = Path(__file__).resolve().parents[2]

# ── deterministic test keys ──────────────────────────────────────────────────
# These match `make_signer()` / `make_signer_b()` in
# crates/donut-adjudicator/src/lib.rs so canonical fixtures reconcile across
# Rust + Python. Never used outside test processes.
PRIV_A = "0x4c0883a69102937d6231471b5dbb6204fe5129617082792ae468d01a3f362318"
PRIV_B = "0xdf57089febbacf7ba0bc227dafbffa9fc08a93fdc67e1e3c5ac6739c1bff21bd"


def make_signer():
    return Account.from_key(PRIV_A)


def make_signer_b():
    return Account.from_key(PRIV_B)


def spinner_id_from_addr(addr: str) -> str:
    """Mirror `donut_adjudicator::spinner_id_from_addr`: lowercase, no 0x,
    first 8 hex chars."""
    return addr.lower().removeprefix("0x")[:8]


# ── Solana chain-id sentinels (mirror the Rust constants) ────────────────────

SOLANA_DST_SENTINEL = 0
SOLANA_DST_WORMHOLE = 1_399_811_149
SOLANA_DST_DEBRIDGE = 100_000_001


def is_solana_dst(c: int) -> bool:
    return c in (SOLANA_DST_SENTINEL, SOLANA_DST_WORMHOLE, SOLANA_DST_DEBRIDGE)


def is_solana_involved(src_chain: int, dst_chain: int) -> bool:
    return is_solana_dst(src_chain) or is_solana_dst(dst_chain)


def expected_adapter_id(protocol: str, src_chain: int, dst_chain: int) -> str:
    """Python mirror of `adapter_id_for_outcome` so tests can pre-compute
    what the server should resolve to."""
    p = protocol.lower()
    solana = is_solana_involved(src_chain, dst_chain)
    if p == "mayan_flash" or ("flash" in p and "mayan" in p):
        return "mayan-flash-solana-v1" if solana else "mayan-flash-evm-v1"
    if p.startswith("mayan"):
        return "mayan-solana-swift-v1" if solana else "mayan-evm-swift-v1"
    if "wormhole" in p or p == "ntt" or "ntt" in p:
        return "wormhole-ntt-solana-v1"
    if p == "lifi":
        return "lifi-meta-v2"
    if p == "across":
        return "across-v3"
    if p == "debridge" or p == "dln" or "debridge" in p or "dln" in p:
        return "debridge-dln-solana-v1" if solana else "debridge-dln-v1"
    return f"unknown-{p}-{dst_chain}"


# ── SIWE ─────────────────────────────────────────────────────────────────────

def build_siwe_message(
    address: str,
    domain: str,
    nonce: str,
    chain_id: int = 1,
    ttl_seconds: int = 300,
    statement: str = (
        "Sign to provision a Taifoon solver pod. This signature is used to "
        "prove address ownership and is not a transaction. No funds are moved."
    ),
) -> str:
    """Build an EIP-4361 SIWE message string. The server parses this via
    the `siwe` Rust crate which is strict about formatting — keep the
    field order and the leading blank lines exactly as below."""
    iat = datetime.now(timezone.utc).replace(microsecond=0)
    exp = iat + timedelta(seconds=ttl_seconds)
    # Address must be EIP-55 checksummed for siwe-rs to parse.
    checksum_addr = to_checksum_address(address)
    iat_str = iat.isoformat().replace("+00:00", "Z")
    exp_str = exp.isoformat().replace("+00:00", "Z")
    msg = (
        f"{domain} wants you to sign in with your Ethereum account:\n"
        f"{checksum_addr}\n\n"
        f"{statement}\n\n"
        f"URI: https://{domain}\n"
        f"Version: 1\n"
        f"Chain ID: {chain_id}\n"
        f"Nonce: {nonce}\n"
        f"Issued At: {iat_str}\n"
        f"Expiration Time: {exp_str}"
    )
    return msg


def sign_siwe(message: str, signer) -> str:
    """personal_sign over the SIWE message bytes. Returns 0x-prefixed hex."""
    encoded = encode_defunct(text=message)
    signed = signer.sign_message(encoded)
    return signed.signature.hex() if signed.signature.hex().startswith("0x") else "0x" + signed.signature.hex()


# ── DonutAttestation construction & signing ──────────────────────────────────

ZERO_HASH = "0x" + "0" * 64

# Back-compat float constants. Kept for tests that just want to express
# the human-friendly fraction; not used in any signing path (we delegate
# signing to the Rust binary which uses integer micro-USD math).
DONUT_BPS = 0.0049
CREATOR_FRAC = 0.70
REVIEWER_FRAC = 0.20
ECOSYSTEM_FRAC = 0.10

# Integer micro-USD constants — mirror donut_adjudicator's MICRO_USD_PER_USD
# and the bps numerator/denominator. Use these for byte-stable expected
# math in Python tests.
MICRO_USD_PER_USD = 1_000_000
DONUT_BPS_NUM = 49
DONUT_BPS_DEN = 10_000
CREATOR_NUM = 70
REVIEWER_NUM = 20
ECOSYSTEM_NUM = 10
SPLIT_DEN = 100


def compute_split_micro(
    fee_micro: int,
    bps_num: int = DONUT_BPS_NUM,
    bps_den: int = DONUT_BPS_DEN,
) -> tuple[int, int, int, int, int]:
    """Integer mirror of Rust's `compute_split_micro`. The donut base is
    `fee_micro` (the SSE-decoded fee, not net profit). `bps_num` and
    `bps_den` default to the canonical 49/10_000 — pass an override when
    a per-adapter rate applies. Ecosystem absorbs the residual so the
    three shares sum to donut_take exactly."""
    positive = max(fee_micro, 0)
    donut = positive * bps_num // bps_den
    creator = donut * CREATOR_NUM // SPLIT_DEN
    reviewer = donut * REVIEWER_NUM // SPLIT_DEN
    ecosystem = donut - creator - reviewer
    keeps = fee_micro - donut
    return donut, creator, reviewer, ecosystem, keeps


def expected_split_micro(
    fee_usd: float,
    bps_num: int = DONUT_BPS_NUM,
    bps_den: int = DONUT_BPS_DEN,
) -> tuple[int, int, int, int, int]:
    """Caller-convenience wrapper: take a USD float (the same value the
    test feeds into `produce_attestation`'s `actual_profit_usd`, which
    the testbin then mirrors into `fee_usd` for sandbox simplicity) and
    return the expected `(donut, creator, reviewer, ecosystem, keeps)`
    in micro-USD."""
    fee_micro = round(fee_usd * MICRO_USD_PER_USD)
    return compute_split_micro(fee_micro, bps_num, bps_den)


def get_ledger_head(base_url: str, spinner_id: str) -> str:
    """Fetch the current prev_hash for a spinner from the ledger head endpoint.
    Returns ZERO_HASH if the spinner has never attested before."""
    try:
        url = f"{base_url}/api/donut/ledger/{spinner_id}/head"
        r = requests.get(url, timeout=2.0)
        if r.status_code == 200:
            data = r.json()
            prev = data.get("prev_hash", ZERO_HASH)
            print(f"[DEBUG] Fetched prev_hash for {spinner_id}: {prev[:18]}...")
            return prev
        # 404 or other error - spinner has no attestations yet
        print(f"[DEBUG] No ledger for {spinner_id} (status {r.status_code})")
        return ZERO_HASH
    except Exception as e:
        # Silently return ZERO_HASH for first attestation case
        print(f"[DEBUG] get_ledger_head failed for {spinner_id}: {e}")
        return ZERO_HASH


def produce_attestation(
    *,
    priv_key_hex: str,
    intent_id: str,
    tx_hash: str,
    protocol: str,
    src_chain: int,
    dst_chain: int,
    actual_profit_usd: float,
    creator_addr: str,
    reviewer_addrs: list[str],
    ecosystem_addr: str,
    prev_hash: str = ZERO_HASH,
    server_url: str | None = None,
    spinner_addr: str | None = None,
) -> dict:
    """Shell out to `solver-api-testbin sign-attestation` to obtain a
    canonical-JSON-signed DonutAttestation.

    If `server_url` and `spinner_addr` are provided and `prev_hash` is
    ZERO_HASH, automatically fetches the current ledger head from the
    server to maintain proper hash chaining across tests.

    Returns a dict with three top-level keys:
        - `attestation`: the signed DonutAttestation, ready to POST to
          `/api/donut/attest`.
        - `signing_preimage`: the exact byte sequence the signer signed
          (useful for diagnosing signature mismatches).
        - `next_prev_hash`: the sha256(canonical_full(attestation)) value
          to use as `prev_hash` on the NEXT attestation from the same
          Spinner.

    The Rust binary owns the canonical-JSON serialization so the Python
    test rig never has to reproduce `serde_json`'s float formatter — which
    is a recipe for off-by-one-bit signature failures.
    """
    # Auto-fetch prev_hash if server context is provided and prev_hash wasn't
    # explicitly set by the caller (still at default ZERO_HASH)
    if server_url and spinner_addr:
        spinner_id = spinner_id_from_addr(spinner_addr)
        print(f"[DEBUG] Auto-fetching for spinner_id={spinner_id} from {spinner_addr}")
        fetched_prev = get_ledger_head(server_url, spinner_id)
        # Use fetched value if available, otherwise keep the passed/default prev_hash
        if fetched_prev and fetched_prev != ZERO_HASH:
            print(f"[DEBUG] Using fetched prev_hash: {fetched_prev[:18]}...")
            prev_hash = fetched_prev
        else:
            print(f"[DEBUG] No prev_hash fetched, using default: {prev_hash[:18]}...")
    spec = {
        "priv_key_hex": priv_key_hex,
        "intent_id": intent_id,
        "tx_hash": tx_hash,
        "protocol": protocol,
        "src_chain": src_chain,
        "dst_chain": dst_chain,
        "actual_profit_usd": actual_profit_usd,
        "creator_addr": creator_addr,
        "reviewer_addrs": reviewer_addrs,
        "ecosystem_addr": ecosystem_addr,
        "prev_hash": prev_hash,
    }
    proc = subprocess.run(
        [
            "cargo",
            "run",
            "-q",
            "-p",
            "solver-api",
            "--bin",
            "solver-api-testbin",
            "--",
            "sign-attestation",
        ],
        input=json.dumps(spec),
        capture_output=True,
        text=True,
        cwd=WORKSPACE,
        check=False,
    )
    if proc.returncode != 0:
        raise RuntimeError(
            "sign-attestation failed:\n"
            f"stdout:\n{proc.stdout}\nstderr:\n{proc.stderr}"
        )
    # The binary may print build logs before the JSON; take only the last
    # non-empty line, which is the envelope.
    for line in reversed(proc.stdout.strip().splitlines()):
        if line.startswith("{"):
            return json.loads(line)
    raise RuntimeError(
        f"sign-attestation produced no JSON output:\nstdout:\n{proc.stdout}"
    )
