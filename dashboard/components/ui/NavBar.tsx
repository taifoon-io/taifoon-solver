'use client'
import Link from 'next/link'
import { usePathname } from 'next/navigation'
import { cn } from './cn'

const links = [
  { href: '/', label: 'HOME' },
  { href: '/portal', label: 'PORTAL' },
  { href: '/t3rn', label: 'LWC' },
  { href: '/onboard', label: 'ONBOARD' },
  { href: '/docs', label: 'DOCS' },
]

/**
 * Nav follows taifoon.io's pattern:
 *  - Triangle/peak mark + TAIFOON wordmark in Inter
 *  - Mono ALL-CAPS link labels (13px, wide tracking)
 *  - Bracketed terminal-style CTA on the right
 */
export function NavBar() {
  const pathname = usePathname()
  return (
    <header className="sticky top-0 z-40 backdrop-blur-md bg-black/80 border-b border-[var(--border-subtle)]">
      <div className="max-w-[1400px] mx-auto px-6 h-14 flex items-center justify-between">
        <Link href="/" className="flex items-center gap-2.5 group">
          <Mark />
          <div className="flex items-baseline gap-2">
            <span className="text-[15px] font-medium tracking-[0.16em] text-[var(--text-primary)]">
              TAIFOON
            </span>
            <span className="text-[11px] font-mono tracking-[0.24em] text-[var(--brand-blue)]">
              / SOLVERS
            </span>
          </div>
        </Link>

        <nav className="hidden md:flex items-center gap-6">
          {links.map((l) => {
            const active =
              l.href === '/' ? pathname === '/' : pathname?.startsWith(l.href)
            return (
              <Link
                key={l.href}
                href={l.href}
                className={cn(
                  'font-mono text-[13px] tracking-[0.16em] transition-colors',
                  active
                    ? 'text-[var(--brand-blue)]'
                    : 'text-[var(--text-secondary)] hover:text-[var(--text-primary)]',
                )}
              >
                {l.label}
              </Link>
            )
          })}
        </nav>

        <div className="flex items-center gap-3">
          <a
            href="https://github.com/yawningmonsoon/taifoon-solver"
            target="_blank"
            rel="noreferrer"
            className="hidden sm:inline-flex font-mono text-[12px] tracking-[0.16em] text-[var(--text-tertiary)] hover:text-[var(--text-primary)] transition-colors"
          >
            GITHUB ↗
          </a>
          <Link
            href="/onboard"
            className="font-mono text-[12px] tracking-[0.16em] text-[var(--brand-blue)] border border-[var(--brand-blue)]/40 hover:border-[var(--brand-blue)] hover:bg-[var(--brand-blue)]/10 px-3 h-8 inline-flex items-center rounded-[var(--r-sm)] transition-all"
          >
            <span className="text-[var(--text-tertiary)] mr-1">{'>'}</span>
            SPIN_UP
            <span className="text-[var(--text-tertiary)] ml-2">▼</span>
          </Link>
        </div>
      </div>
    </header>
  )
}

function Mark() {
  // Minimal triangular peak — same family as the taifoon.io mark, slightly
  // sharper / brighter to fit the Solana-friendly tone.
  return (
    <svg width="22" height="22" viewBox="0 0 24 24" fill="none" aria-hidden="true">
      <path
        d="M2 20 L12 4 L22 20 L18 20 L12 11 L6 20 Z"
        fill="url(#mark-grad)"
      />
      <defs>
        <linearGradient id="mark-grad" x1="0" y1="0" x2="24" y2="24">
          <stop offset="0%" stopColor="#3DA5FF" />
          <stop offset="100%" stopColor="#14F195" />
        </linearGradient>
      </defs>
    </svg>
  )
}
