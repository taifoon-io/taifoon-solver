# ─── Build stage ────────────────────────────────────────────────────────
# Compiles the Rust workspace and produces the `taifoon-solver` binary.
# Pinned to rust:1.83 to match the workspace's edition / toolchain.
FROM rust:1.83 AS builder

WORKDIR /build

# Install build-time deps for the native libs (RocksDB, etc.) the
# workspace's transitive deps pull in. clang + libclang-dev satisfy
# bindgen for any *-sys crates.
RUN apt-get update \
 && apt-get install -y --no-install-recommends \
      pkg-config libssl-dev clang libclang-dev cmake \
 && rm -rf /var/lib/apt/lists/*

# Copy the entire workspace. The .dockerignore at the repo root keeps
# target/ and node_modules/ out of the build context.
COPY . .

# Build only the binary the deploy box ships — keeps the image surface
# small and the build time bounded.
RUN cargo build --release --bin taifoon-solver \
 && strip /build/target/release/taifoon-solver

# ─── Runtime stage ──────────────────────────────────────────────────────
# Ubuntu 24.04 picks up GLIBC 2.39, matching the production host the
# operator typically targets. Non-root user (`spinner`) with $HOME at
# /home/spinner so the SQLite + key mounts have a deterministic owner.
FROM ubuntu:24.04 AS runtime

RUN apt-get update \
 && apt-get install -y --no-install-recommends \
      ca-certificates libssl3 tini curl \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --create-home --shell /usr/sbin/nologin --uid 10001 spinner \
 && mkdir -p /data/outcomes /data/keychain /config \
 && chown -R spinner:spinner /data /config

COPY --from=builder /build/target/release/taifoon-solver /usr/local/bin/taifoon-solver

# Sample registry file copied into the image so a fresh box without a
# mounted config still has SOMETHING to read (fail-closed ZERO
# ecosystem — the operator MUST mount a real config before going live).
COPY --chown=spinner:spinner config/adapter_registry.json /config/adapter_registry.json

USER spinner
WORKDIR /home/spinner

# Live upstream defaults — public DNS only. The operator's onboarding
# flow at solver.taifoon.dev/onboard hands out the matching
# SOLVER_API_TOKEN.
ENV SPINNER_API_URL=https://api.taifoon.dev \
    GENOME_SSE_URL=https://api.taifoon.dev/api/genome/subscribe/sse \
    ADAPTER_REGISTRY_PATH=/config/adapter_registry.json \
    HOSTING_DB_PATH=/data/outcomes/hosting.sqlite \
    OUTCOME_DB_PATH=/data/outcomes/outcomes.sqlite \
    DRY_RUN=true \
    RUST_LOG=info \
    PORT=8082

# Health endpoint exposed on $PORT. The box exposes the solver-api
# surface on 8082 by default (matches the dashboard's
# NEXT_PUBLIC_SOLVER_API_URL).
EXPOSE 8082

# tini reaps any zombies the solver might leave behind during shutdown.
ENTRYPOINT ["/usr/bin/tini", "--"]
CMD ["/usr/local/bin/taifoon-solver"]

HEALTHCHECK --interval=30s --timeout=5s --start-period=20s --retries=3 \
  CMD curl -fsS http://127.0.0.1:${PORT:-8082}/health || exit 1
