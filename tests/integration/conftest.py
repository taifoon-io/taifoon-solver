"""
Pytest fixtures for the Taifoon solver integration rig.

Boots `solver-api-testbin` against a fresh temp SQLite + a random token,
yields the base URL + token + DB path so individual tests can exercise the
live HTTP surface.

Run:
    cd /Users/mbaj/projects/taifoon-solver
    python3 -m pip install -r tests/integration/requirements.txt
    pytest tests/integration -v
"""
from __future__ import annotations

import os
import socket
import secrets
import subprocess
import tempfile
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Iterator

import pytest
import requests

WORKSPACE = Path(__file__).resolve().parents[2]


def _find_free_port() -> int:
    """Find a free TCP port on 127.0.0.1 — avoids clashing with a running
    `solver-main` or another rig instance on the dev box."""
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


@dataclass
class TestServer:
    base_url: str
    token: str
    db_path: str


@pytest.fixture(scope="session")
def workspace() -> Path:
    return WORKSPACE


@pytest.fixture(scope="session")
def server(workspace: Path) -> Iterator[TestServer]:
    """Build (cached) and run the test binary; tear it down at session end."""
    # Pre-build so subsequent `cargo run` is instant. Failures surface
    # compile errors before the first test runs, with full output.
    build = subprocess.run(
        [
            "cargo",
            "build",
            "-p",
            "solver-api",
            "--bin",
            "solver-api-testbin",
        ],
        cwd=workspace,
        capture_output=True,
        text=True,
    )
    if build.returncode != 0:
        raise RuntimeError(
            "cargo build solver-api-testbin failed:\n"
            f"stdout:\n{build.stdout}\nstderr:\n{build.stderr}"
        )

    tmp = tempfile.TemporaryDirectory(prefix="taifoon-rig-")
    db = str(Path(tmp.name) / "hosting.sqlite")
    token = secrets.token_hex(16)
    port = _find_free_port()

    env = {
        **os.environ,
        "HOSTING_DB_PATH": db,
        "SOLVER_API_TOKEN": token,
        "PORT": str(port),
        "RUST_LOG": "warn",
    }

    # `cargo run -q` is quiet enough that stdout/stderr aren't noisy under
    # `-v` pytest. If a test fails and you need server logs, set
    # RUST_LOG=info before invoking pytest — the env merge above propagates.
    proc = subprocess.Popen(
        [
            "cargo",
            "run",
            "-q",
            "-p",
            "solver-api",
            "--bin",
            "solver-api-testbin",
        ],
        cwd=workspace,
        env=env,
    )

    base = f"http://127.0.0.1:{port}"
    deadline = time.time() + 30.0
    last_err: Exception | None = None
    while time.time() < deadline:
        if proc.poll() is not None:
            raise RuntimeError(f"test server exited early with code {proc.returncode}")
        try:
            r = requests.get(f"{base}/health", timeout=1.0)
            if r.status_code == 200:
                break
        except Exception as e:  # noqa: BLE001
            last_err = e
        time.sleep(0.25)
    else:
        proc.terminate()
        raise RuntimeError(
            f"test server did not respond on {base}/health within 30s "
            f"(last err: {last_err!r})"
        )

    try:
        yield TestServer(base_url=base, token=token, db_path=db)
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait(timeout=2)
        tmp.cleanup()


@pytest.fixture
def auth_headers(server: TestServer) -> dict:
    """Bearer-token headers for protected mutation routes."""
    return {"Authorization": f"Bearer {server.token}"}
