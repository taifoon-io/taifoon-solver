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

from eth_account import Account
from eth_account.messages import encode_defunct

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
    checksum_addr = Account.to_checksum_address(address)
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
DONUT_BPS = 0.0049
CREATOR_FRAC = 0.70
REVIEWER_FRAC = 0.20
ECOSYSTEM_FRAC = 0.10


def compute_split(actual_profit_usd: float) -> tuple[float, float, float, float, float]:
    profit = actual_profit_usd if actual_profit_usd == actual_profit_usd else 0.0  # filter NaN
    positive = max(profit, 0.0)
    donut = positive * DONUT_BPS
    creator = donut * CREATOR_FRAC
    reviewer = donut * REVIEWER_FRAC
    ecosystem = donut * ECOSYSTEM_FRAC
    keeps = profit - donut
    return donut, creator, reviewer, ecosystem, keeps


def _canonical_json(obj: dict, exclude: tuple = ()) -> str:
    """Sort keys recursively, no whitespace — mirrors Rust's `sort_value`
    + `serde_json::to_string`. The exclude tuple drops top-level keys
    (used to remove `signature_hex` from the signing pre-image)."""
    def normalize(v):
        if isinstance(v, dict):
            return {k: normalize(v[k]) for k in sorted(v.keys())}
        if isinstance(v, list):
            return [normalize(x) for x in v]
        return v
    pruned = {k: v for k, v in obj.items() if k not in exclude}
    return json.dumps(normalize(pruned), separators=(",", ":"), sort_keys=True)


def _serialize_float(x: float) -> float | int:
    """serde_json prints integers like 1.0 as `1`. Python's json prints `1.0`.
    For byte-stable matching with the Rust signing pre-image, coerce
    whole-number floats to int (the Rust side does the same via
    serde_json's default Number serialization)."""
    if x == int(x):
        return int(x)
    return x


def _attestation_payload(
    fill_id: str,
    spinner_addr: str,
    adapter_id: str,
    protocol: str,
    dst_chain: int,
    actual_profit_usd: float,
    creator_addr: str,
    reviewer_addrs: list[str],
    ecosystem_addr: str,
    ts_iso: str,
    prev_hash: str,
) -> dict:
    donut, creator, reviewer, ecosystem, keeps = compute_split(actual_profit_usd)
    spinner_id = spinner_id_from_addr(spinner_addr)
    return {
        "fill_id": fill_id,
        "spinner_id": spinner_id,
        "spinner_addr": spinner_addr.lower(),
        "adapter_id": adapter_id,
        "protocol": protocol,
        "dst_chain": dst_chain,
        "actual_profit_usd": actual_profit_usd,
        "donut_take_usd": donut,
        "creator_addr": creator_addr.lower(),
        "creator_share_usd": creator,
        "reviewer_addrs": [a.lower() for a in reviewer_addrs],
        "reviewer_share_usd": reviewer,
        "ecosystem_addr": ecosystem_addr.lower(),
        "ecosystem_share_usd": ecosystem,
        "spinner_keeps_usd": keeps,
        "ts": ts_iso,
        "prev_hash": prev_hash,
        "signature_hex": "",  # populated by `sign_attestation`
    }


def sign_attestation(payload: dict, signer) -> dict:
    """Compute the canonical signing JSON, EIP-191 personal_sign it, and
    return the same payload with `signature_hex` populated. Matches the
    Rust `CanonicalAdjudicator::attest` byte-for-byte."""
    signing_json = _canonical_json(payload, exclude=("signature_hex",))
    encoded = encode_defunct(text=signing_json)
    signed = signer.sign_message(encoded)
    sig_hex = signed.signature.hex()
    if not sig_hex.startswith("0x"):
        sig_hex = "0x" + sig_hex
    payload["signature_hex"] = sig_hex
    return payload


def hash_for_chain(payload: dict) -> str:
    """sha256 of the canonical JSON *including* signature — drives the
    next attestation's `prev_hash`."""
    s = _canonical_json(payload, exclude=())
    return "0x" + hashlib.sha256(s.encode("utf-8")).hexdigest()


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
) -> dict:
    """Shell out to `solver-api-testbin sign-attestation` to obtain a
    canonical-JSON-signed DonutAttestation.

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
