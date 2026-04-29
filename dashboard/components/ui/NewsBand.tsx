'use client'
import Link from 'next/link'
import { useEffect, useState } from 'react'

/**
 * NewsBand — a thin, dismissable banner above the nav. Used for
 * time-bound announcements: hackathons, mainnet flips, talks, grants.
 *
 * Design contract:
 *   - 32px tall, mono micro-caps text, hairline-bordered
 *   - One pill-style "tag" on the left (e.g. NEW · HACKATHON · LIVE)
 *   - Headline + optional href + optional date
 *   - "×" dismiss → remembers in sessionStorage by id
 *   - Animated entrance (slide down)
 *
 * Adding a new entry: edit `NEWS` below and bump its id.
 */

export interface NewsItem {
  id: string
  /** Pill text, displayed in mono ALL CAPS in the band's accent color. */
  pill: string
  pillTone?: 'mint' | 'blue' | 'violet'
  /** Single-line headline. Keep it short. */
  headline: string
  /** Optional click target. Internal routes use Link; external uses anchor. */
  href?: string
  external?: boolean
  /** Optional ISO date for "13 days left" ticker, etc. */
  endsAt?: string
}

const NEWS: NewsItem[] = [
  {
    id: 'colosseum-2026',
    pill: 'Hackathon',
    pillTone: 'mint',
    headline:
      'Live on Solana Colosseum — open-source cross-chain solver runtime for hackathon teams.',
    href: 'https://www.colosseum.org',
    external: true,
    endsAt: '2026-06-15',
  },
  // Future entries go above this comment. The most recent (top) item is shown.
  // {
  //   id: 'mainnet-2026q3',
  //   pill: 'Mainnet',
  //   pillTone: 'blue',
  //   headline: 'Mainnet flip — 31 protocols on Base + Solana, fee distribution live.',
  //   href: '/portal',
  // },
]

const STORAGE_KEY_PREFIX = 'taifoon-news-dismiss:'

export function NewsBand() {
  const [item, setItem] = useState<NewsItem | null>(null)
  const [mounted, setMounted] = useState(false)

  useEffect(() => {
    // Deferred initial state so React 19's set-state-in-effect rule
    // doesn't flag this — the band is intentionally client-only to
    // avoid hydration mismatches with sessionStorage.
    queueMicrotask(() => {
      setMounted(true)
      const candidate = NEWS[0]
      if (!candidate) return
      try {
        const dismissed = sessionStorage.getItem(STORAGE_KEY_PREFIX + candidate.id)
        if (!dismissed) setItem(candidate)
      } catch {
        setItem(candidate)
      }
    })
  }, [])

  if (!mounted || !item) return null

  const dismiss = () => {
    try {
      sessionStorage.setItem(STORAGE_KEY_PREFIX + item.id, '1')
    } catch {
      /* no-op */
    }
    setItem(null)
  }

  const pillClass =
    item.pillTone === 'blue'
      ? 'text-[var(--brand-blue)] border-[var(--brand-blue)]/40'
      : item.pillTone === 'violet'
      ? 'text-[var(--solana-violet)] border-[var(--solana-violet)]/40'
      : 'text-[var(--solana-mint)] border-[var(--solana-mint)]/40'

  const daysLeft = item.endsAt ? daysUntil(item.endsAt) : null

  const inner = (
    <>
      <span
        className={`inline-flex items-center px-2 h-5 rounded-[2px] border font-mono text-[10px] tracking-[0.24em] uppercase ${pillClass}`}
      >
        {item.pill}
      </span>
      <span className="text-[var(--text-secondary)] font-mono text-[12px] truncate">
        {item.headline}
      </span>
      {daysLeft !== null && daysLeft >= 0 && (
        <span className="hidden md:inline-flex font-mono text-[10px] tracking-[0.16em] text-[var(--text-tertiary)] uppercase shrink-0">
          {daysLeft === 0 ? 'last day' : `${daysLeft}d left`}
        </span>
      )}
      {item.href && (
        <span className="font-mono text-[10px] tracking-[0.2em] uppercase text-[var(--brand-blue)] shrink-0">
          READ ↗
        </span>
      )}
    </>
  )

  return (
    <div
      className="relative border-b border-[var(--border-subtle)] bg-[var(--bg-base)]"
      style={{ animation: 'newsband-slide 400ms var(--ease-out) both' }}
    >
      <div className="max-w-[1400px] mx-auto px-6 h-9 flex items-center gap-3">
        <span className="font-mono text-[10px] tracking-[0.24em] text-[var(--text-tertiary)] uppercase shrink-0 hidden sm:inline">
          [ NEWS ]
        </span>
        <div className="flex-1 min-w-0 flex items-center gap-3">
          {item.href ? (
            item.external ? (
              <a
                href={item.href}
                target="_blank"
                rel="noreferrer"
                className="flex items-center gap-3 min-w-0 flex-1 hover:opacity-80 transition-opacity"
              >
                {inner}
              </a>
            ) : (
              <Link
                href={item.href}
                className="flex items-center gap-3 min-w-0 flex-1 hover:opacity-80 transition-opacity"
              >
                {inner}
              </Link>
            )
          ) : (
            <div className="flex items-center gap-3 min-w-0 flex-1">{inner}</div>
          )}
        </div>
        <button
          aria-label="Dismiss news"
          onClick={dismiss}
          className="font-mono text-[14px] leading-none text-[var(--text-tertiary)] hover:text-[var(--text-primary)] transition-colors shrink-0 ml-2"
        >
          ×
        </button>
      </div>
      <style>{`
        @keyframes newsband-slide {
          from { transform: translateY(-100%); opacity: 0; }
          to { transform: translateY(0); opacity: 1; }
        }
      `}</style>
    </div>
  )
}

function daysUntil(iso: string): number | null {
  try {
    const target = new Date(iso).getTime()
    const now = Date.now()
    const ms = target - now
    if (ms < 0) return -1
    return Math.ceil(ms / (1000 * 60 * 60 * 24))
  } catch {
    return null
  }
}
