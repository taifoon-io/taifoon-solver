import { ReactNode } from 'react'
import { cn } from './cn'

interface CardProps {
  children: ReactNode
  className?: string
  /** Adds a subtle gradient border for emphasis. */
  glow?: boolean
  /** Padding scale; default 'md'. */
  padding?: 'none' | 'sm' | 'md' | 'lg'
}

const padMap = {
  none: '',
  sm: 'p-3',
  md: 'p-5',
  lg: 'p-7',
}

export function Card({ children, className, glow, padding = 'md' }: CardProps) {
  return (
    <div
      className={cn(
        'relative rounded-[var(--r-lg)] bg-[var(--bg-elevated)] border border-[var(--border-default)]',
        'shadow-[var(--shadow-card)]',
        padMap[padding],
        glow && 'before:absolute before:inset-0 before:rounded-[var(--r-lg)] before:p-[1px] before:-z-10 before:bg-gradient-to-br before:from-[var(--brand-cyan)]/40 before:to-[var(--brand-violet)]/40',
        className,
      )}
    >
      {children}
    </div>
  )
}

interface CardHeaderProps {
  title: string
  subtitle?: string
  trailing?: ReactNode
  className?: string
}

export function CardHeader({ title, subtitle, trailing, className }: CardHeaderProps) {
  return (
    <div className={cn('flex items-start justify-between gap-3 mb-4', className)}>
      <div className="min-w-0">
        <div className="text-[10px] font-bold uppercase tracking-[0.18em] text-[var(--text-tertiary)]">
          {title}
        </div>
        {subtitle && <div className="mt-1 text-xs text-[var(--text-secondary)]">{subtitle}</div>}
      </div>
      {trailing && <div className="shrink-0">{trailing}</div>}
    </div>
  )
}
