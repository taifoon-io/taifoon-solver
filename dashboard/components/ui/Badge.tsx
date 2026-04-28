import { ReactNode } from 'react'
import { cn } from './cn'

type Tone = 'neutral' | 'success' | 'warning' | 'danger' | 'info' | 'violet'

const toneMap: Record<Tone, string> = {
  neutral: 'bg-[var(--bg-raised)] text-[var(--text-secondary)] border-[var(--border-default)]',
  success: 'bg-[#00FF8810] text-[var(--success)] border-[#00FF8833]',
  warning: 'bg-[#FFB80014] text-[var(--warning)] border-[#FFB80038]',
  danger:  'bg-[#FF336614] text-[var(--danger)] border-[#FF336638]',
  info:    'bg-[#00D9FF14] text-[var(--brand-cyan)] border-[#00D9FF38]',
  violet:  'bg-[#9945FF14] text-[var(--brand-violet)] border-[#9945FF38]',
}

interface BadgeProps {
  children: ReactNode
  tone?: Tone
  dot?: boolean
  className?: string
  pulse?: boolean
}

export function Badge({ children, tone = 'neutral', dot, pulse, className }: BadgeProps) {
  return (
    <span
      className={cn(
        'inline-flex items-center gap-1.5 px-2 py-0.5 rounded-[var(--r-pill)] border',
        'text-[10px] font-mono uppercase tracking-wider',
        toneMap[tone],
        className,
      )}
    >
      {dot && (
        <span
          className={cn('w-1.5 h-1.5 rounded-full bg-current', pulse && 'animate-pulse')}
        />
      )}
      {children}
    </span>
  )
}
