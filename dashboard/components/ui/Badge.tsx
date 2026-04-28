import { ReactNode } from 'react'
import { cn } from './cn'

type Tone = 'neutral' | 'success' | 'warning' | 'danger' | 'info' | 'mint' | 'violet'

const toneMap: Record<Tone, string> = {
  neutral: 'border-[var(--border-default)] text-[var(--text-secondary)]',
  success: 'border-[var(--success)]/40 text-[var(--success)]',
  warning: 'border-[var(--warning)]/40 text-[var(--warning)]',
  danger:  'border-[var(--danger)]/40 text-[var(--danger)]',
  info:    'border-[var(--brand-blue)]/40 text-[var(--brand-blue)]',
  mint:    'border-[var(--solana-mint)]/40 text-[var(--solana-mint)]',
  violet:  'border-[var(--solana-violet)]/40 text-[var(--solana-violet)]',
}

interface BadgeProps {
  children: ReactNode
  tone?: Tone
  dot?: boolean
  className?: string
  pulse?: boolean
}

/**
 * Badge — outlined, mono, ALL-CAPS, wide tracking. No fill background to
 * stay aligned with taifoon.io's flat aesthetic.
 */
export function Badge({ children, tone = 'neutral', dot, pulse, className }: BadgeProps) {
  return (
    <span
      className={cn(
        'inline-flex items-center gap-1.5 px-2 py-1 rounded-[var(--r-sm)] border',
        'text-[10px] font-mono uppercase tracking-[0.2em]',
        toneMap[tone],
        className,
      )}
    >
      {dot && (
        <span
          className={cn(
            'w-1.5 h-1.5 rounded-full bg-current',
            pulse && 'animate-pulse',
          )}
        />
      )}
      {children}
    </span>
  )
}
