'use client'

import { NavBar, Footer, Card, CardHeader, Button, Snippet, Tag } from '@/components/ui'

export default function DocsPage() {
  return (
    <>
      <NavBar />
      <main className="flex-1">
        <div className="max-w-[920px] mx-auto px-6 py-16">
          <Tag>Docs</Tag>
          <h1 className="tf-display tf-gradient-silver mt-4 text-[clamp(2rem,4vw,3rem)]">
            Solver runtime
            <br />
            in three pages.
          </h1>
          <p className="mt-5 text-[var(--text-secondary)] leading-relaxed">
            Lightweight pointers to the source. Full documentation lives in
            the repo at{' '}
            <a
              href="https://github.com/yawningmonsoon/taifoon-solver"
              target="_blank"
              rel="noreferrer"
              className="text-[var(--brand-blue)] hover:underline"
            >
              yawningmonsoon/taifoon-solver
            </a>
            .
          </p>

          <div className="mt-12 space-y-5">
            {/* ── Quick start ──────────────────────────────────────────── */}
            <Card padding="md">
              <CardHeader title="Quick start" />
              <Snippet
                variant="tabbed"
                tabs={[
                  {
                    label: 'CLONE',
                    code: 'git clone https://github.com/yawningmonsoon/taifoon-solver\ncd taifoon-solver',
                  },
                  {
                    label: 'BUILD',
                    code: 'cargo build --release -p taifoon-solver',
                  },
                  {
                    label: 'RUN',
                    code: 'cargo run --release -p taifoon-solver -- --stream prod',
                  },
                ]}
              />
            </Card>

            {/* ── Environment variables ────────────────────────────────── */}
            <Card padding="md">
              <CardHeader title="Environment variables" />
              <p className="text-sm text-[var(--text-secondary)] leading-relaxed mb-4">
                Copy{' '}
                <code className="text-[var(--brand-blue)] font-mono text-[12px]">.env.example</code>
                {' '}to{' '}
                <code className="text-[var(--brand-blue)] font-mono text-[12px]">.env</code>
                {' '}and fill in the values before starting the solver.
              </p>
              <Snippet
                variant="default"
                lang="env"
                code={`SOLVER_PRIVATE_KEY=0x...          # EVM hot wallet private key
SOLVER_ADDRESS=0x...            # Corresponding EVM address
MAX_NOTIONAL_USD=5000           # Per-fill exposure cap in USD
MIN_PROFIT_USD=0.10             # Skip fills below this profit threshold
API_PORT=8082                   # Solver HTTP API + dashboard port`}
              />
            </Card>

            {/* ── Keychain setup ───────────────────────────────────────── */}
            <Card padding="md">
              <CardHeader title="Keychain setup (macOS)" />
              <p className="text-sm text-[var(--text-secondary)] leading-relaxed mb-4">
                Store credentials in the system keychain instead of a plaintext{' '}
                <code className="text-[var(--brand-blue)] font-mono text-[12px]">.env</code>
                {' '}file. The solver reads from the{' '}
                <code className="text-[var(--brand-blue)] font-mono text-[12px]">mamba-messiah-key</code>
                {' '}service name at startup.
              </p>
              <Snippet
                variant="tabbed"
                tabs={[
                  {
                    label: 'STORE KEY',
                    code: 'security add-generic-password \\\n  -s mamba-messiah-key \\\n  -a solver \\\n  -w "$(cat ~/.solver_private_key)"',
                  },
                  {
                    label: 'VERIFY',
                    code: 'security find-generic-password -s mamba-messiah-key -w',
                  },
                  {
                    label: 'DELETE',
                    code: 'security delete-generic-password -s mamba-messiah-key',
                  },
                ]}
              />
            </Card>

            {/* ── Architecture ─────────────────────────────────────────── */}
            <Card padding="md">
              <CardHeader title="Architecture" />
              <p className="text-sm text-[var(--text-secondary)] leading-relaxed">
                A solver is a single Rust process composed of a genome-stream client,
                a profitability calculator, a wallet manager, a protocol-adapter
                layer (per-protocol calldata builders), and an executor. Each pod
                owns one wallet and one set of protocol/chain registrations.
                The sidecar portfolio manager exposes inventory and P&amp;L via the
                HTTP API on{' '}
                <code className="text-[var(--brand-blue)] font-mono text-[12px]">
                  localhost:{'{'}API_PORT{'}'}/api/solver/*
                </code>
                .
              </p>
            </Card>

            {/* ── Lambda lifecycle ─────────────────────────────────────── */}
            <Card padding="md">
              <CardHeader title="Lambda lifecycle" />
              <p className="text-sm text-[var(--text-secondary)] leading-relaxed">
                Every intent flows through a 12-stage state machine — from{' '}
                <code className="text-[var(--brand-blue)] font-mono">detected</code>{' '}
                to{' '}
                <code className="text-[var(--solana-mint)] font-mono">confirmed</code>{' '}
                or one of the terminal failure stages. The portal renders this in
                real time from server-sent events.
              </p>
              <div className="mt-4">
                <Button href="/portal" variant="primary" size="sm">
                  SEE IT LIVE →
                </Button>
              </div>
            </Card>
          </div>
        </div>
      </main>
      <Footer />
    </>
  )
}
