'use client'
import Link from 'next/link'
import { usePathname } from 'next/navigation'
import { Button } from './Button'
import { cn } from './cn'

const links = [
  { href: '/', label: 'Home' },
  { href: '/portal', label: 'Portal' },
  { href: '/onboard', label: 'Onboard' },
  { href: '/docs', label: 'Docs' },
]

export function NavBar() {
  const pathname = usePathname()
  return (
    <header className="sticky top-0 z-40 backdrop-blur-md bg-[var(--bg-base)]/70 border-b border-[var(--border-subtle)]">
      <div className="max-w-[1400px] mx-auto px-6 h-14 flex items-center justify-between">
        <Link href="/" className="flex items-center gap-2.5 group">
          <Logo />
          <div className="flex flex-col leading-none">
            <span className="text-[13px] font-bold tracking-wide">
              <span className="text-[var(--text-primary)]">solver</span>
              <span className="text-[var(--text-tertiary)]">.taifoon.dev</span>
            </span>
            <span className="text-[9px] text-[var(--text-tertiary)] uppercase tracking-[0.2em] mt-0.5">
              cross-chain solver runtime
            </span>
          </div>
        </Link>

        <nav className="hidden md:flex items-center gap-1">
          {links.map((l) => {
            const active = l.href === '/' ? pathname === '/' : pathname?.startsWith(l.href)
            return (
              <Link
                key={l.href}
                href={l.href}
                className={cn(
                  'px-3 h-8 rounded-[var(--r-md)] text-[12px] font-medium flex items-center transition-colors',
                  active
                    ? 'text-[var(--brand-cyan)] bg-[var(--bg-elevated)]'
                    : 'text-[var(--text-secondary)] hover:text-[var(--text-primary)] hover:bg-[var(--bg-elevated)]',
                )}
              >
                {l.label}
              </Link>
            )
          })}
        </nav>

        <div className="flex items-center gap-2">
          <Button href="https://github.com/yawningmonsoon/taifoon-solver" external variant="ghost" size="sm">
            GitHub
          </Button>
          <Button href="/onboard" variant="glow" size="sm">
            Spin up solver →
          </Button>
        </div>
      </div>
    </header>
  )
}

function Logo() {
  return (
    <div className="relative w-8 h-8 rounded-[var(--r-md)] grid place-items-center bg-gradient-to-br from-[var(--brand-cyan)] to-[var(--brand-violet)] shadow-[var(--glow-cyan)]">
      <svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="black" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
        <path d="M3 12 L9 12 L11 6 L13 18 L15 12 L21 12" />
      </svg>
    </div>
  )
}
