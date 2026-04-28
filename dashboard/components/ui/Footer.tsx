import Link from 'next/link'

/**
 * Footer mirrors taifoon.io's quiet, terminal-flavored bottom strip.
 * One column of brand context, three columns of mono link lists.
 */
export function Footer() {
  return (
    <footer className="border-t border-[var(--border-subtle)] mt-32">
      <div className="max-w-[1400px] mx-auto px-6 py-12 grid gap-10 md:grid-cols-4">
        <div>
          <div className="flex items-baseline gap-2 mb-3">
            <span className="text-[14px] font-medium tracking-[0.16em]">TAIFOON</span>
            <span className="text-[10px] font-mono tracking-[0.24em] text-[var(--brand-blue)]">
              / SPINNERS
            </span>
          </div>
          <p className="text-xs text-[var(--text-secondary)] leading-relaxed">
            The autonomous spinner runtime. One process, every protocol,
            Solana and EVM under the same root.
          </p>
        </div>

        <Col title="PRODUCT">
          <FLink href="/portal">Portal</FLink>
          <FLink href="/onboard">Onboarding</FLink>
          <FLink href="/docs">Docs</FLink>
          <FLink href="https://taifoon.io" external>
            taifoon.io
          </FLink>
        </Col>

        <Col title="PROTOCOLS">
          <span className="text-[var(--text-secondary)] text-xs">Across V3</span>
          <span className="text-[var(--text-secondary)] text-xs">deBridge DLN</span>
          <span className="text-[var(--text-secondary)] text-xs">Mayan Swift</span>
          <span className="text-[var(--text-secondary)] text-xs">LiFi · Stargate · CCTP</span>
          <span className="text-[var(--text-tertiary)] text-xs">+ 26 more</span>
        </Col>

        <Col title="HACKATHON">
          <FLink href="https://www.colosseum.org" external>
            Solana Colosseum
          </FLink>
          <FLink href="https://github.com/yawningmonsoon/taifoon-solver" external>
            GitHub
          </FLink>
          <FLink href="mailto:hello@taifoon.dev">hello@taifoon.dev</FLink>
        </Col>
      </div>
      <div className="border-t border-[var(--border-subtle)]">
        <div className="max-w-[1400px] mx-auto px-6 py-4 flex items-center justify-between flex-wrap gap-2">
          <span className="text-[10px] font-mono tracking-[0.24em] text-[var(--text-tertiary)]">
            © TAIFOON · MIT
          </span>
          <span className="text-[10px] font-mono tracking-[0.16em] text-[var(--text-tertiary)]">
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
      <div className="tf-tag mb-4">[ {title} ]</div>
      <div className="flex flex-col gap-2.5">{children}</div>
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
        className="text-xs text-[var(--text-secondary)] hover:text-[var(--brand-blue)] transition-colors"
      >
        {children} ↗
      </a>
    )
  }
  return (
    <Link
      href={href}
      className="text-xs text-[var(--text-secondary)] hover:text-[var(--brand-blue)] transition-colors"
    >
      {children}
    </Link>
  )
}
