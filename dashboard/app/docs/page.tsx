'use client'

import { NavBar, Footer, Card, CardHeader, Button, Badge, CodeBlock } from '@/components/ui'

export default function DocsPage() {
  return (
    <>
      <NavBar />
      <main className="flex-1">
        <div className="max-w-[920px] mx-auto px-6 py-12">
          <Badge tone="info">Docs</Badge>
          <h1 className="mt-3 text-3xl font-bold tracking-tight">solver.taifoon.dev — runtime overview</h1>
          <p className="mt-3 text-[var(--text-secondary)]">
            Lightweight pointers to the source. Full documentation lives in the
            repo at{' '}
            <a
              href="https://github.com/yawningmonsoon/taifoon-solver"
              target="_blank"
              rel="noreferrer"
              className="text-[var(--brand-cyan)]"
            >
              yawningmonsoon/taifoon-solver
            </a>
            .
          </p>

          <div className="mt-10 space-y-5">
            <Card padding="md">
              <CardHeader title="Quick start" />
              <CodeBlock
                lang="bash"
                code={`cargo install taifoon-cli
taifoon-cli onboard --chains base,solana --protocols across,debridge,mayan
taifoon-cli run --stream prod`}
              />
            </Card>

            <Card padding="md">
              <CardHeader title="Architecture" />
              <p className="text-sm text-[var(--text-secondary)] leading-relaxed">
                A solver is a single Rust process composed of a genome-stream client,
                a profitability calculator, a wallet manager, a protocol-adapter
                layer (per-protocol calldata builders), and an executor. Each pod
                owns one wallet and one set of protocol/chain registrations.
              </p>
            </Card>

            <Card padding="md">
              <CardHeader title="Lambda lifecycle" />
              <p className="text-sm text-[var(--text-secondary)] leading-relaxed">
                Every intent flows through a 12-stage state machine — from{' '}
                <code className="text-[var(--brand-cyan)] font-mono">detected</code>{' '}
                to{' '}
                <code className="text-[var(--success)] font-mono">confirmed</code>{' '}
                or one of the terminal failure stages. The portal renders this in
                real time from server-sent events.
              </p>
              <div className="mt-3">
                <Button href="/portal" variant="secondary" size="sm">
                  See it live
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
