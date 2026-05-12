"""
User-journey integration tests for the Taifoon solver hosting + donut
attestation surface.

Every test is named after a question a real user lands on
`solver.taifoon.dev` already asking:

    - Can I onboard?
    - Are my funds safe?
    - Are my funds actively filling intents to the biggest extent?
    - Is the loop wired together for the TSUL 70 / 20 / 10 split?
    - What happens if I run an unregistered adapter?
    - Can someone else write to my ledger?

The rig boots `solver-api-testbin` in a subprocess against a fresh SQLite
DB (see `conftest.py::server`), then drives the HTTP surface with the
same wire format the dashboard uses. Attestation signing is delegated to
the Rust binary via `helpers.produce_attestation()` so float
serialization drift never invalidates a signature.
"""
from __future__ import annotations

import json
import os
import sqlite3

import requests

from helpers import (
    build_siwe_message,
    expected_adapter_id,
    expected_split_micro,
    get_ledger_head,
    make_signer,
    make_signer_b,
    produce_attestation,
    sign_siwe,
    spinner_id_from_addr,
    PRIV_A,
    PRIV_B,
    SOLANA_DST_DEBRIDGE,
    SOLANA_DST_WORMHOLE,
    ZERO_HASH,
    MICRO_USD_PER_USD,
)

# ── Addresses used across tests ──────────────────────────────────────────────
ECOSYSTEM_ADDR = "0x000000000000000000000000000000000000eEEe"
MAYAN_SWIFT_SOLANA_BUILDER = "0x111111111111111111111111111111111111aaaa"
MAYAN_FLASH_SOLANA_BUILDER = "0x222222222222222222222222222222222222bbbb"
WORMHOLE_NTT_SOLANA_BUILDER = "0x333333333333333333333333333333333333cccc"
DEBRIDGE_DLN_SOLANA_BUILDER = "0x444444444444444444444444444444444444dddd"
REVIEWER_ADDRS = [
    "0x000000000000000000000000000000000000aaaa",
    "0x000000000000000000000000000000000000bbbb",
]


# ── 1. Can I onboard? ────────────────────────────────────────────────────────

def test_can_i_onboard_with_siwe(server):
    """User lands on solver.taifoon.dev. Connects wallet. Signs a SIWE
    message. Provisions. Receives solver_id + api_token. Their hosting
    row exists and `siwe_verified=1`."""
    signer = make_signer()

    # Step 1 — issue a SIWE nonce for this address.
    r = requests.post(
        f"{server.base_url}/api/hosting/siwe-nonce",
        json={"address": signer.address.lower()},
    )
    assert r.status_code == 200, r.text
    nonce_body = r.json()
    assert "nonce" in nonce_body and len(nonce_body["nonce"]) == 64
    assert nonce_body["ttl_seconds"] == 300
    nonce = nonce_body["nonce"]

    # Step 2 — construct SIWE message and sign.
    domain = os.environ.get("SIWE_DOMAIN", "solver.taifoon.dev")
    msg = build_siwe_message(
        address=signer.address,
        domain=domain,
        nonce=nonce,
    )
    sig = sign_siwe(msg, signer)

    # Step 3 — provision.
    r = requests.post(
        f"{server.base_url}/api/hosting/provision",
        json={
            "name": "test-onboarding-solver",
            "evm_address": signer.address.lower(),
            "siwe_message": msg,
            "signature": sig,
        },
    )
    assert r.status_code == 200, r.text
    body = r.json()
    solver_id = body["solver_id"]
    api_token = body["api_token"]
    assert solver_id == spinner_id_from_addr(signer.address)
    assert len(api_token) == 48  # 24 bytes hex
    assert body["portal_url"].endswith(f"/portal/{solver_id}")

    # Step 4 — confirm the row exists, siwe_verified=1.
    r = requests.get(f"{server.base_url}/api/hosting/solvers/{solver_id}")
    assert r.status_code == 200, r.text
    solver = r.json()
    assert solver["evm_address"] == signer.address.lower()
    assert solver["donut_accrued_usd_micro"] == 0  # no fills yet
    assert solver["active"] is True


def test_provision_without_siwe_is_marked_unverified(server):
    """Backward-compat path: a provision without SIWE still creates the
    row but `siwe_verified=0` — dashboard can show a 'Verify your wallet'
    CTA. Verified by reading the SQLite directly."""
    signer = make_signer_b()
    r = requests.post(
        f"{server.base_url}/api/hosting/provision",
        json={
            "name": "no-siwe-solver",
            "evm_address": signer.address.lower(),
        },
    )
    assert r.status_code == 200, r.text
    solver_id = r.json()["solver_id"]

    # Read the column straight out of SQLite.
    conn = sqlite3.connect(server.db_path)
    row = conn.execute(
        "SELECT siwe_verified FROM hosted_solvers WHERE solver_id = ?",
        (solver_id,),
    ).fetchone()
    conn.close()
    assert row is not None
    assert row[0] == 0


# ── 2. Are my funds safe? ────────────────────────────────────────────────────

def test_my_funds_are_safe(server, auth_headers):
    """Three things to prove:
       (a) no signing-key material ever leaves the server,
       (b) the api_token is stored hashed, not in plaintext,
       (c) a tampered api_token gets a 401."""
    signer = make_signer()

    # Provision (no SIWE — that's covered by test_can_i_onboard_with_siwe).
    r = requests.post(
        f"{server.base_url}/api/hosting/provision",
        json={"name": "funds-safe", "evm_address": signer.address.lower()},
    )
    assert r.status_code == 200
    solver_id = r.json()["solver_id"]
    api_token = r.json()["api_token"]

    # (a) GET /api/hosting/solvers/:id must not leak any signing key, raw
    # api_token, private key, mnemonic, etc.
    r = requests.get(f"{server.base_url}/api/hosting/solvers/{solver_id}")
    payload = json.dumps(r.json())
    for forbidden in [
        "private_key",
        "privkey",
        "mnemonic",
        api_token,  # the raw token must NEVER appear in any GET response
        PRIV_A,
        PRIV_B,
    ]:
        assert forbidden.lower() not in payload.lower(), f"leak: {forbidden}"

    # (b) The DB has only the hashed token.
    conn = sqlite3.connect(server.db_path)
    row = conn.execute(
        "SELECT api_token_hash FROM hosted_solvers WHERE solver_id = ?",
        (solver_id,),
    ).fetchone()
    conn.close()
    stored_hash = row[0]
    assert stored_hash != api_token
    assert len(stored_hash) > 0

    # (c) Hit a token-gated endpoint with a tampered token → 401.
    bad_headers = {"Authorization": "Bearer " + "x" * len(api_token)}
    r = requests.get(f"{server.base_url}/api/solver/outcomes", headers=bad_headers)
    assert r.status_code == 401, f"expected 401 with bad token, got {r.status_code}: {r.text}"

    # And with the right token (the server's master token from conftest),
    # token-gated routes succeed.
    r = requests.get(f"{server.base_url}/api/solver/outcomes", headers=auth_headers)
    # 200 with empty array or 500 if outcome_log isn't injected — accept
    # either, since the bearer gate is what we're testing here, not the
    # downstream handler's full wiring.
    assert r.status_code in (200, 500), r.text


# ── 3. Are my funds actively filling intents? ────────────────────────────────

def test_funds_actively_filling_intents_across_solana_protocols(server, auth_headers):
    """Story: I'm running a Spinner. I fill four Solana intents (one per
    Solana protocol). Each fill emits a signed attestation. The ledger
    reports them in order. The hash chain links across all four. The
    portal `donut_accrued_usd` increments ONLY when I'm the Builder of
    the adapter I'm running."""
    signer = make_signer()
    # Provision so my Spinner row exists.
    r = requests.post(
        f"{server.base_url}/api/hosting/provision",
        json={"name": "active-spinner", "evm_address": signer.address.lower()},
    )
    assert r.status_code == 200
    solver_id = r.json()["solver_id"]

    # One fill per Solana protocol — same pattern as the Rust sandbox tests.
    fills = [
        # (protocol, src_chain, dst_chain, profit, builder_addr)
        ("mayan_swift", SOLANA_DST_WORMHOLE, 1, 0.10, MAYAN_SWIFT_SOLANA_BUILDER),
        ("mayan_flash", SOLANA_DST_WORMHOLE, 8453, 0.20, MAYAN_FLASH_SOLANA_BUILDER),
        ("wormhole_ntt", 8453, SOLANA_DST_WORMHOLE, 0.30, WORMHOLE_NTT_SOLANA_BUILDER),
        ("debridge_dln", 1, SOLANA_DST_DEBRIDGE, 0.40, DEBRIDGE_DLN_SOLANA_BUILDER),
    ]

    prev_hash = ZERO_HASH
    posted = []
    for i, (protocol, src, dst, profit, builder) in enumerate(fills):
        env = produce_attestation(
            priv_key_hex=PRIV_A,
            intent_id=f"intent-{i}",
            tx_hash=f"0xtx{i:064x}",
            protocol=protocol,
            src_chain=src,
            dst_chain=dst,
            actual_profit_usd=profit,
            creator_addr=builder,
            reviewer_addrs=REVIEWER_ADDRS,
            ecosystem_addr=ECOSYSTEM_ADDR,
            prev_hash=prev_hash,
        )
        att = env["attestation"]
        # Sanity-check the resolved adapter_id agrees with our Python mirror.
        assert att["adapter_id"] == expected_adapter_id(protocol, src, dst)

        r = requests.post(
            f"{server.base_url}/api/donut/attest",
            json=att,
            headers=auth_headers,
        )
        assert r.status_code == 200, f"protocol={protocol} resp={r.status_code}:{r.text}"
        posted.append(att)
        prev_hash = env["next_prev_hash"]

    # Read the ledger — 4 rows, in order.
    r = requests.get(f"{server.base_url}/api/donut/ledger/{solver_id}")
    assert r.status_code == 200, r.text
    ledger = r.json()["attestations"]
    assert len(ledger) == 4, f"expected 4 ledger rows, got {len(ledger)}"
    for stored, expected in zip(ledger, posted):
        assert stored["fill_id"] == expected["fill_id"]
        assert stored["adapter_id"] == expected["adapter_id"]
        assert stored["spinner_addr"].lower() == signer.address.lower()

    # The Spinner is NOT any of these Builders → donut_accrued stays 0.
    r = requests.get(f"{server.base_url}/api/hosting/solvers/{solver_id}")
    assert r.status_code == 200
    assert r.json()["donut_accrued_usd_micro"] == 0


def test_spinner_who_is_also_the_builder_accrues_donut(server, auth_headers):
    """If a Spinner ships their OWN adapter and runs it, the 70%
    creator-share lands on their hosting row's `donut_accrued_usd`."""
    signer = make_signer()
    # Provision.
    r = requests.post(
        f"{server.base_url}/api/hosting/provision",
        json={"name": "builder-spinner", "evm_address": signer.address.lower()},
    )
    assert r.status_code == 200
    solver_id = r.json()["solver_id"]

    # Single fill where the Spinner IS the Builder.
    profit = 1.00
    env = produce_attestation(
        priv_key_hex=PRIV_A,
        intent_id="builder-fill-1",
        tx_hash="0xself",
        protocol="mayan_swift",
        src_chain=SOLANA_DST_WORMHOLE,
        dst_chain=1,
        actual_profit_usd=profit,
        creator_addr=signer.address.lower(),  # ← Spinner is Builder
        reviewer_addrs=REVIEWER_ADDRS,
        ecosystem_addr=ECOSYSTEM_ADDR,
        server_url=server.base_url,
        spinner_addr=signer.address,
    )
    r = requests.post(
        f"{server.base_url}/api/donut/attest",
        json=env["attestation"],
        headers=auth_headers,
    )
    assert r.status_code == 200, r.text

    # donut_accrued should equal the creator's share, in micro-USD.
    _, expected_creator, _, _, _ = expected_split_micro(profit)
    r = requests.get(f"{server.base_url}/api/hosting/solvers/{solver_id}")
    accrued = r.json()["donut_accrued_usd_micro"]
    assert accrued == expected_creator, f"accrued={accrued}, expected={expected_creator}"


# ── 4. Is the TSUL 70 / 20 / 10 split wired together end-to-end? ─────────────

def test_tsul_70_20_10_split_is_correct(server, auth_headers):
    """For a synthetic $100 profit fill the attestation must say (in micro-USD):
       - donut_take_usd_micro      = 490_000      (49 bps of $100 = $0.49)
       - creator_share_usd_micro   = 343_000      (70% of donut)
       - reviewer_share_usd_micro  =  98_000      (20%)
       - ecosystem_share_usd_micro =  49_000      (10%)
       - shares sum to donut_take exactly (integer math).
    And the same shape must round-trip from POST → GET ledger byte-stably."""
    signer = make_signer()
    # Provision.
    requests.post(
        f"{server.base_url}/api/hosting/provision",
        json={"name": "split-test", "evm_address": signer.address.lower()},
    )
    solver_id = spinner_id_from_addr(signer.address)

    profit = 100.0
    env = produce_attestation(
        priv_key_hex=PRIV_A,
        intent_id="split-fill",
        tx_hash="0xsplit",
        protocol="mayan_swift",
        src_chain=SOLANA_DST_WORMHOLE,
        dst_chain=1,
        actual_profit_usd=profit,
        creator_addr=MAYAN_SWIFT_SOLANA_BUILDER,
        reviewer_addrs=REVIEWER_ADDRS,
        ecosystem_addr=ECOSYSTEM_ADDR,
        server_url=server.base_url,
        spinner_addr=signer.address,
    )
    att = env["attestation"]

    # Math checks — what the SIGNER computed.
    assert att["donut_take_usd_micro"] == 490_000, att["donut_take_usd_micro"]
    assert att["creator_share_usd_micro"] == 343_000, att["creator_share_usd_micro"]
    assert att["reviewer_share_usd_micro"] == 98_000, att["reviewer_share_usd_micro"]
    assert att["ecosystem_share_usd_micro"] == 49_000, att["ecosystem_share_usd_micro"]
    share_sum = (
        att["creator_share_usd_micro"]
        + att["reviewer_share_usd_micro"]
        + att["ecosystem_share_usd_micro"]
    )
    assert share_sum == att["donut_take_usd_micro"]
    assert att["spinner_keeps_usd_micro"] == 100 * MICRO_USD_PER_USD - 490_000

    # POST it and confirm the served ledger has the same numbers.
    r = requests.post(
        f"{server.base_url}/api/donut/attest",
        json=att,
        headers=auth_headers,
    )
    assert r.status_code == 200, r.text

    r = requests.get(f"{server.base_url}/api/donut/ledger/{solver_id}")
    assert r.status_code == 200
    served = r.json()["attestations"][0]
    for key in (
        "donut_take_usd_micro",
        "creator_share_usd_micro",
        "reviewer_share_usd_micro",
        "ecosystem_share_usd_micro",
        "spinner_keeps_usd_micro",
    ):
        assert served[key] == att[key], f"{key} drift: {served[key]} vs {att[key]}"


def test_tsul_split_is_correct_for_every_solana_protocol(server, auth_headers):
    """Same 70/20/10 invariant must hold for every Solana protocol path
    we route — including the Solana-source-EVM-dst direction that pre-patch
    mis-routed Mayan to the EVM Builder."""
    signer = make_signer()
    requests.post(
        f"{server.base_url}/api/hosting/provision",
        json={"name": "split-all-protos", "evm_address": signer.address.lower()},
    )
    solver_id = spinner_id_from_addr(signer.address)

    fills = [
        ("mayan_swift", SOLANA_DST_WORMHOLE, 1, 0.42, MAYAN_SWIFT_SOLANA_BUILDER, "mayan-solana-swift-v1"),
        ("mayan_swift", 8453, SOLANA_DST_WORMHOLE, 0.55, MAYAN_SWIFT_SOLANA_BUILDER, "mayan-solana-swift-v1"),
        ("mayan_flash", SOLANA_DST_WORMHOLE, 8453, 0.75, MAYAN_FLASH_SOLANA_BUILDER, "mayan-flash-solana-v1"),
        ("wormhole_ntt", 8453, SOLANA_DST_WORMHOLE, 0.30, WORMHOLE_NTT_SOLANA_BUILDER, "wormhole-ntt-solana-v1"),
        ("debridge_dln", 1, SOLANA_DST_DEBRIDGE, 0.20, DEBRIDGE_DLN_SOLANA_BUILDER, "debridge-dln-solana-v1"),
    ]

    prev = ZERO_HASH
    for i, (protocol, src, dst, profit, builder, want_adapter) in enumerate(fills):
        env = produce_attestation(
            priv_key_hex=PRIV_A,
            intent_id=f"all-{i}",
            tx_hash=f"0xall{i}",
            protocol=protocol,
            src_chain=src,
            dst_chain=dst,
            actual_profit_usd=profit,
            creator_addr=builder,
            reviewer_addrs=REVIEWER_ADDRS,
            ecosystem_addr=ECOSYSTEM_ADDR,
            prev_hash=prev,
        )
        att = env["attestation"]
        assert att["adapter_id"] == want_adapter, (
            f"protocol={protocol} resolved {att['adapter_id']}, expected {want_adapter}"
        )
        # 70/20/10 integer micro-USD math.
        e_donut, e_creator, e_reviewer, e_ecosystem, _ = expected_split_micro(profit)
        assert att["donut_take_usd_micro"] == e_donut
        assert att["creator_share_usd_micro"] == e_creator
        assert att["reviewer_share_usd_micro"] == e_reviewer
        assert att["ecosystem_share_usd_micro"] == e_ecosystem

        r = requests.post(
            f"{server.base_url}/api/donut/attest",
            json=att,
            headers=auth_headers,
        )
        assert r.status_code == 200, f"protocol={protocol} resp={r.status_code}:{r.text}"
        prev = env["next_prev_hash"]

    # The full ledger contains 5 rows in order, with adapter_ids in the
    # expected order.
    r = requests.get(f"{server.base_url}/api/donut/ledger/{solver_id}")
    served = r.json()["attestations"]
    assert len(served) == 5
    assert [row["adapter_id"] for row in served] == [f[5] for f in fills]


# ── 5. Negative: what if I run an unregistered adapter? ──────────────────────

def test_unregistered_adapter_routes_donut_to_ecosystem(server, auth_headers):
    """Fail-closed path — when an attestation lands for an adapter the
    Spinner OS doesn't recognise, the 70% Builder cut goes to the
    ecosystem treasury, not silently to the Spinner. We simulate this by
    asking `produce_attestation` to claim ecosystem_addr as the Builder
    (the Rust binary's empty-registry path produces this exact shape)."""
    signer = make_signer()
    requests.post(
        f"{server.base_url}/api/hosting/provision",
        json={"name": "unregistered", "evm_address": signer.address.lower()},
    )
    solver_id = spinner_id_from_addr(signer.address)

    env = produce_attestation(
        priv_key_hex=PRIV_A,
        intent_id="unreg-1",
        tx_hash="0xunreg",
        protocol="mayan_swift",
        src_chain=SOLANA_DST_WORMHOLE,
        dst_chain=1,
        actual_profit_usd=1.0,
        creator_addr=ECOSYSTEM_ADDR,  # ← simulating unregistered fallback
        reviewer_addrs=[ECOSYSTEM_ADDR],
        ecosystem_addr=ECOSYSTEM_ADDR,
        server_url=server.base_url,
        spinner_addr=signer.address,
    )
    r = requests.post(
        f"{server.base_url}/api/donut/attest",
        json=env["attestation"],
        headers=auth_headers,
    )
    assert r.status_code == 200, r.text

    # Spinner's row should NOT have accrued any donut — they're not the
    # ecosystem.
    r = requests.get(f"{server.base_url}/api/hosting/solvers/{solver_id}")
    assert r.json()["donut_accrued_usd_micro"] == 0


# ── 6. Can someone else write to my ledger? ──────────────────────────────────

def test_spinner_cannot_post_attestation_for_another_spinner(server, auth_headers):
    """HIGH-severity audit regression: a Spinner with a valid API token
    must NOT be able to POST an attestation that claims another
    Spinner's `spinner_id`. After the fix in hosting.rs::persist_attestation,
    the server derives `spinner_id` from the recovered signature address
    and rejects mismatched bodies."""
    # Both Spinners provision themselves so the rows exist.
    signer_a = make_signer()
    signer_b_ = make_signer_b()
    for s in (signer_a, signer_b_):
        requests.post(
            f"{server.base_url}/api/hosting/provision",
            json={"name": f"binding-{s.address[:10]}", "evm_address": s.address.lower()},
        )

    # Spinner A produces a perfectly valid attestation signed by their own key.
    env = produce_attestation(
        priv_key_hex=PRIV_A,
        intent_id="binding-1",
        tx_hash="0xbind",
        protocol="mayan_swift",
        src_chain=SOLANA_DST_WORMHOLE,
        dst_chain=1,
        actual_profit_usd=0.50,
        creator_addr=MAYAN_SWIFT_SOLANA_BUILDER,
        reviewer_addrs=REVIEWER_ADDRS,
        ecosystem_addr=ECOSYSTEM_ADDR,
        server_url=server.base_url,
        spinner_addr=signer_a.address,
    )
    att = env["attestation"]

    # Tamper with the body's `spinner_id` to point at Spinner B's id while
    # the signature still recovers to Spinner A.
    att["spinner_id"] = spinner_id_from_addr(signer_b_.address)

    r = requests.post(
        f"{server.base_url}/api/donut/attest",
        json=att,
        headers=auth_headers,
    )
    # Must be rejected (400) — the binding check inside persist_attestation
    # catches that att.spinner_id doesn't equal short_id(att.spinner_addr).
    assert r.status_code == 400, (
        f"expected 400 (binding rejection), got {r.status_code}: {r.text}"
    )
    body = r.json()
    assert "spinner_id" in body.get("detail", ""), body


def test_attest_route_requires_bearer_token(server):
    """Without a bearer token, POST /api/donut/attest must fail with 401
    before any signature inspection happens."""
    signer = make_signer()
    requests.post(
        f"{server.base_url}/api/hosting/provision",
        json={"name": "no-auth", "evm_address": signer.address.lower()},
    )
    env = produce_attestation(
        priv_key_hex=PRIV_A,
        intent_id="no-auth-1",
        tx_hash="0xnoauth",
        protocol="mayan_swift",
        src_chain=SOLANA_DST_WORMHOLE,
        dst_chain=1,
        actual_profit_usd=0.10,
        creator_addr=MAYAN_SWIFT_SOLANA_BUILDER,
        reviewer_addrs=REVIEWER_ADDRS,
        ecosystem_addr=ECOSYSTEM_ADDR,
        server_url=server.base_url,
        spinner_addr=signer.address,
    )
    r = requests.post(
        f"{server.base_url}/api/donut/attest",
        json=env["attestation"],
        # no headers
    )
    assert r.status_code == 401, r.text


def test_attest_duplicate_fill_id_rejected(server, auth_headers):
    """Replaying the same fill must 409 — primary-key constraint on
    fill_id stops it. Confirms the audit's 'looks-fine' replay-protection
    holds against an HTTP-level retry."""
    signer = make_signer()
    requests.post(
        f"{server.base_url}/api/hosting/provision",
        json={"name": "dup-test", "evm_address": signer.address.lower()},
    )
    env = produce_attestation(
        priv_key_hex=PRIV_A,
        intent_id="dup-fill",
        tx_hash="0xdup",
        protocol="mayan_swift",
        src_chain=SOLANA_DST_WORMHOLE,
        dst_chain=1,
        actual_profit_usd=0.25,
        creator_addr=MAYAN_SWIFT_SOLANA_BUILDER,
        reviewer_addrs=REVIEWER_ADDRS,
        ecosystem_addr=ECOSYSTEM_ADDR,
        server_url=server.base_url,
        spinner_addr=signer.address,
    )
    r1 = requests.post(
        f"{server.base_url}/api/donut/attest",
        json=env["attestation"],
        headers=auth_headers,
    )
    assert r1.status_code == 200, r1.text
    r2 = requests.post(
        f"{server.base_url}/api/donut/attest",
        json=env["attestation"],
        headers=auth_headers,
    )
    assert r2.status_code == 409, r2.text
    assert r2.json().get("error") == "duplicate_fill_id"
