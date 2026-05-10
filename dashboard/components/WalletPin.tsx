'use client'

/**
 * WalletPin — lets any visitor pin an EVM address to observe its portfolio.
 *
 * Persists the pinned address to localStorage under "taifoon_pinned_wallet".
 * On pin, navigates to /watch?address=0x... so the URL is shareable.
 *
 * No wallet signing required — balances are public on-chain reads.
 * No wagmi / rainbowkit — we stay dependency-free by accepting a paste.
 */

import { useEffect, useRef, useState } from 'react'
import { useRouter } from 'next/navigation'

const STORAGE_KEY = 'taifoon_pinned_wallet'

function isValidEvm(addr: string): boolean {
  return /^0x[0-9a-fA-F]{40}$/.test(addr.trim())
}

function shortAddr(a: string): string {
  return `${a.slice(0, 6)}…${a.slice(-4)}`
}

interface WalletPinProps {
  /** Called after a successful pin so the parent can re-render if needed. */
  onPin?: (address: string) => void
}

export default function WalletPin({ onPin }: WalletPinProps) {
  const router = useRouter()
  const [pinned, setPinned] = useState<string | null>(null)
  const [open, setOpen] = useState(false)
  const [input, setInput] = useState('')
  const [error, setError] = useState<string | null>(null)
  const inputRef = useRef<HTMLInputElement>(null)

  useEffect(() => {
    try {
      const stored = localStorage.getItem(STORAGE_KEY)
      if (stored && isValidEvm(stored)) setPinned(stored)
    } catch {}
  }, [])

  useEffect(() => {
    if (open) setTimeout(() => inputRef.current?.focus(), 50)
  }, [open])

  function handlePin() {
    const addr = input.trim()
    if (!isValidEvm(addr)) {
      setError('Enter a valid 0x… EVM address (42 chars)')
      return
    }
    try { localStorage.setItem(STORAGE_KEY, addr) } catch {}
    setPinned(addr)
    setOpen(false)
    setInput('')
    setError(null)
    onPin?.(addr)
    router.push(`/watch?address=${encodeURIComponent(addr)}`)
  }

  function handleUnpin() {
    try { localStorage.removeItem(STORAGE_KEY) } catch {}
    setPinned(null)
    setOpen(false)
  }

  function handleKey(e: React.KeyboardEvent) {
    if (e.key === 'Enter') handlePin()
    if (e.key === 'Escape') { setOpen(false); setError(null) }
  }

  if (!open) {
    return pinned ? (
      <div className="hidden sm:flex items-center gap-2">
        <button
          onClick={() => router.push(`/watch?address=${encodeURIComponent(pinned)}`)}
          className="flex items-center gap-1.5 font-mono text-[11px] tracking-[0.12em] text-[var(--solana-mint)] border border-[var(--solana-mint)]/30 hover:border-[var(--solana-mint)]/60 px-2.5 h-7 rounded-[var(--r-sm)] transition-all"
          title={pinned}
        >
          <span className="w-1.5 h-1.5 rounded-full bg-[var(--solana-mint)] animate-pulse" />
          {shortAddr(pinned)}
        </button>
        <button
          onClick={handleUnpin}
          className="font-mono text-[10px] text-[var(--text-tertiary)] hover:text-[var(--danger)] transition-colors"
          title="Unpin wallet"
          aria-label="Unpin wallet"
        >
          ✕
        </button>
      </div>
    ) : (
      <button
        onClick={() => setOpen(true)}
        className="hidden sm:inline-flex items-center gap-1.5 font-mono text-[11px] tracking-[0.12em] text-[var(--text-tertiary)] border border-[var(--border-default)] hover:border-[var(--brand-blue)]/50 hover:text-[var(--brand-blue)] px-2.5 h-7 rounded-[var(--r-sm)] transition-all"
      >
        ⬡ PIN WALLET
      </button>
    )
  }

  return (
    <div className="hidden sm:flex items-center gap-1.5">
      <div className="relative">
        <input
          ref={inputRef}
          value={input}
          onChange={(e) => { setInput(e.target.value); setError(null) }}
          onKeyDown={handleKey}
          placeholder="0x… EVM address"
          className="w-[240px] h-7 px-2.5 font-mono text-[11px] tracking-[0.06em] bg-[var(--bg-raised)] border border-[var(--brand-blue)]/50 rounded-[var(--r-sm)] text-[var(--text-primary)] placeholder:text-[var(--text-disabled)] outline-none focus:border-[var(--brand-blue)] transition-colors"
          spellCheck={false}
          autoComplete="off"
        />
        {error && (
          <div className="absolute top-full left-0 mt-1 font-mono text-[9px] text-[var(--danger)] whitespace-nowrap">
            {error}
          </div>
        )}
      </div>
      <button
        onClick={handlePin}
        className="font-mono text-[10px] tracking-[0.14em] text-[var(--brand-blue)] border border-[var(--brand-blue)]/40 hover:border-[var(--brand-blue)] hover:bg-[var(--brand-blue)]/10 px-2 h-7 rounded-[var(--r-sm)] transition-all"
      >
        PIN
      </button>
      <button
        onClick={() => { setOpen(false); setError(null) }}
        className="font-mono text-[10px] text-[var(--text-tertiary)] hover:text-[var(--text-primary)] transition-colors px-1"
        aria-label="Cancel"
      >
        ✕
      </button>
    </div>
  )
}
