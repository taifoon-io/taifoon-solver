'use client'
import Link from 'next/link'
import { usePathname } from 'next/navigation'
import { useState } from 'react'
import WalletPin from '@/components/WalletPin'
import { cn } from './cn'

const links: { href: string; label: string; volt?: boolean }[] = [
  { href: '/', label: 'HOME' },
  { href: '/portal', label: 'PORTAL' },
  { href: '/watch', label: 'MY WALLET' },
  { href: '/analytics', label: 'ANALYTICS' },
  { href: '/builders/bounties', label: 'ROUTES', volt: true },
  { href: '/onboard', label: 'ONBOARD' },
  { href: '/policy', label: 'POLICY' },
  { href: '/docs', label: 'DOCS' },
]

export function NavBar() {
  const pathname = usePathname()
  const [menuOpen, setMenuOpen] = useState(false)

  return (
    <header className="sticky top-0 z-40 backdrop-blur-md bg-black/80 border-b border-[var(--border-subtle)]">
      <div className="max-w-[1400px] mx-auto px-6 h-14 flex items-center justify-between">
        <Link href="/" className="flex items-center gap-2.5 group" onClick={() => setMenuOpen(false)}>
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

        {/* Desktop nav */}
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
                    ? l.volt ? 'text-[var(--solana-mint)]' : 'text-[var(--brand-blue)]'
                    : l.volt
                      ? 'text-[var(--solana-mint)]/70 hover:text-[var(--solana-mint)]'
                      : 'text-[var(--text-secondary)] hover:text-[var(--text-primary)]',
                )}
              >
                {l.label}
              </Link>
            )
          })}
        </nav>

        <div className="flex items-center gap-3">
          <WalletPin />
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
            className="hidden sm:inline-flex font-mono text-[12px] tracking-[0.16em] text-[var(--brand-blue)] border border-[var(--brand-blue)]/40 hover:border-[var(--brand-blue)] hover:bg-[var(--brand-blue)]/10 px-3 h-8 items-center rounded-[var(--r-sm)] transition-all"
          >
            <span className="text-[var(--text-tertiary)] mr-1">{'>'}</span>
            SPIN_UP
            <span className="text-[var(--text-tertiary)] ml-2">▼</span>
          </Link>

          {/* Mobile hamburger */}
          <button
            className="md:hidden flex flex-col justify-center items-center w-8 h-8 gap-1.5"
            onClick={() => setMenuOpen((v) => !v)}
            aria-label={menuOpen ? 'Close menu' : 'Open menu'}
            aria-expanded={menuOpen}
          >
            <span
              className={cn(
                'block w-5 h-px bg-[var(--text-secondary)] transition-all origin-center',
                menuOpen && 'rotate-45 translate-y-[7px]',
              )}
            />
            <span
              className={cn(
                'block w-5 h-px bg-[var(--text-secondary)] transition-all',
                menuOpen && 'opacity-0',
              )}
            />
            <span
              className={cn(
                'block w-5 h-px bg-[var(--text-secondary)] transition-all origin-center',
                menuOpen && '-rotate-45 -translate-y-[7px]',
              )}
            />
          </button>
        </div>
      </div>

      {/* Mobile menu panel */}
      {menuOpen && (
        <div className="md:hidden border-t border-[var(--border-subtle)] bg-black/95 backdrop-blur-md">
          <nav className="max-w-[1400px] mx-auto px-6 py-4 flex flex-col gap-4">
            {links.map((l) => {
              const active =
                l.href === '/' ? pathname === '/' : pathname?.startsWith(l.href)
              return (
                <Link
                  key={l.href}
                  href={l.href}
                  onClick={() => setMenuOpen(false)}
                  className={cn(
                    'font-mono text-[13px] tracking-[0.16em] transition-colors py-1',
                    active
                      ? l.volt ? 'text-[var(--solana-mint)]' : 'text-[var(--brand-blue)]'
                      : l.volt ? 'text-[var(--solana-mint)]/70' : 'text-[var(--text-secondary)]',
                  )}
                >
                  {active ? `[ ${l.label} ]` : l.label}
                </Link>
              )
            })}
            <div className="border-t border-[var(--border-subtle)] pt-3 flex gap-4">
              <a
                href="https://github.com/yawningmonsoon/taifoon-solver"
                target="_blank"
                rel="noreferrer"
                className="font-mono text-[12px] tracking-[0.16em] text-[var(--text-tertiary)] hover:text-[var(--text-primary)] transition-colors"
                onClick={() => setMenuOpen(false)}
              >
                GITHUB ↗
              </a>
              <Link
                href="/onboard"
                onClick={() => setMenuOpen(false)}
                className="font-mono text-[12px] tracking-[0.16em] text-[var(--brand-blue)]"
              >
                {'> SPIN_UP'}
              </Link>
            </div>
          </nav>
        </div>
      )}
    </header>
  )
}

function Mark() {
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
