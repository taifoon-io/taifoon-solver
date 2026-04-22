FROM ubuntu:24.04

# Install runtime dependencies only (no build tools needed)
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

# Copy pre-built binary from local build
COPY target/release/taifoon-solver /usr/local/bin/taifoon-solver
COPY config/solver_intel.json /app/config/solver_intel.json

WORKDIR /app

# Set environment
ENV RUST_LOG=info
ENV GENOME_SSE_URL=https://api.taifoon.dev/api/genome/subscribe/sse
ENV MIN_PROFIT_USD=1.0

# Run solver
CMD ["/usr/local/bin/taifoon-solver"]
