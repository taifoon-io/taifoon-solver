'use client'

import { NavBar, Footer, Card, CardHeader, Button, CodeBlock, Tag } from '@/components/ui'

export default function DocsPage() {
  return (
    <>
      <NavBar />
      <main className="flex-1">
        <div className="max-w-[920px] mx-auto px-6 py-16">
          <Tag>Docs</Tag>
          <h1 className="tf-display tf-gradient-silver mt-4 text-[clamp(2rem,4vw,3rem)]">
            Spinner runtime
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
                A spinner is a single Rust process composed of a genome-stream client,
                a profitability calculator, a wallet manager, a protocol-adapter
                layer (per-protocol calldata builders), and an executor. Each pod
                owns one wallet and one set of protocol/chain registrations.
              </p>
            </Card>

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
