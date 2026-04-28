import Link from 'next/link'

export function Footer() {
  return (
    <footer className="border-t border-[var(--border-subtle)] mt-24">
      <div className="max-w-[1400px] mx-auto px-6 py-10 grid gap-8 md:grid-cols-4">
        <div>
          <div className="text-[13px] font-bold tracking-wide">
            <span className="text-[var(--text-primary)]">solver</span>
            <span className="text-[var(--text-tertiary)]">.taifoon.dev</span>
          </div>
          <p className="mt-2 text-xs text-[var(--text-secondary)] leading-relaxed">
            The fastest open-source cross-chain solver runtime. Built for hackathon
            operators, production market-makers, and everything in between.
          </p>
        </div>

        <Col title="Product">
          <FLink href="/portal">Portal</FLink>
          <FLink href="/onboard">Onboarding</FLink>
          <FLink href="/docs">Docs</FLink>
          <FLink href="https://taifoon.io" external>taifoon.io ↗</FLink>
        </Col>

        <Col title="Protocols">
          <span className="text-[var(--text-secondary)]">Across V3</span>
          <span className="text-[var(--text-secondary)]">deBridge DLN</span>
          <span className="text-[var(--text-secondary)]">Mayan Swift</span>
          <span className="text-[var(--text-secondary)]">LiFi · Stargate · CCTP</span>
          <span className="text-[var(--text-tertiary)]">+ 26 more</span>
        </Col>

        <Col title="Hackathon">
          <FLink href="https://www.colosseum.org" external>Solana Colosseum ↗</FLink>
          <FLink href="https://github.com/yawningmonsoon/taifoon-solver" external>GitHub ↗</FLink>
          <FLink href="mailto:hello@taifoon.dev">hello@taifoon.dev</FLink>
        </Col>
      </div>
      <div className="border-t border-[var(--border-subtle)]">
        <div className="max-w-[1400px] mx-auto px-6 py-4 flex items-center justify-between">
          <span className="text-[10px] text-[var(--text-tertiary)] uppercase tracking-[0.2em]">
            © Taifoon · MIT License
          </span>
          <span className="text-[10px] text-[var(--text-tertiary)] font-mono">
            v0.1 · 31 protocols · 38+ chains
          </span>
        </div>
      </div>
    </footer>
  )
}

function Col({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div>
      <div className="text-[10px] font-bold uppercase tracking-[0.2em] text-[var(--text-tertiary)] mb-3">
        {title}
      </div>
      <div className="flex flex-col gap-2 text-xs">{children}</div>
    </div>
  )
}

function FLink({
  href,
  children,
  external,
}: {
  href: string
  children: React.ReactNode
  external?: boolean
}) {
  if (external) {
    return (
      <a
        href={href}
        target="_blank"
        rel="noreferrer"
        className="text-[var(--text-secondary)] hover:text-[var(--brand-cyan)] transition-colors"
      >
        {children}
      </a>
    )
  }
  return (
    <Link
      href={href}
      className="text-[var(--text-secondary)] hover:text-[var(--brand-cyan)] transition-colors"
    >
      {children}
    </Link>
  )
}
