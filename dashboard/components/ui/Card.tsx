import { ReactNode } from 'react'
import { cn } from './cn'

interface CardProps {
  children: ReactNode
  className?: string
  /** Adds an azure top-line to nominate a card as primary content. */
  accent?: boolean
  padding?: 'none' | 'sm' | 'md' | 'lg'
}

const padMap = {
  none: '',
  sm: 'p-3',
  md: 'p-5',
  lg: 'p-7',
}

/**
 * Surface primitive. Hairline border, no shadow. The taifoon.io aesthetic
 * keeps backgrounds nearly invisible — borders do most of the work.
 */
export function Card({ children, className, accent, padding = 'md' }: CardProps) {
  return (
    <div
      className={cn(
        'relative rounded-[var(--r-sm)] bg-[var(--bg-elevated)] border border-[var(--border-default)]',
        padMap[padding],
        accent && 'before:absolute before:inset-x-0 before:top-0 before:h-px before:bg-[var(--brand-blue)]',
        className,
      )}
    >
      {children}
    </div>
  )
}

interface CardHeaderProps {
  /** Bracketed tag label, e.g. "[ INTENT STREAM ]". */
  title: string
  subtitle?: string
  trailing?: ReactNode
  className?: string
  /** Whether to wrap the title in [ ] brackets. Default true. */
  bracketed?: boolean
}

export function CardHeader({
  title,
  subtitle,
  trailing,
  className,
  bracketed = true,
}: CardHeaderProps) {
  const display = bracketed ? `[ ${title} ]` : title
  return (
    <div className={cn('flex items-start justify-between gap-3 mb-4', className)}>
      <div className="min-w-0">
        <div className="tf-tag">{display}</div>
        {subtitle && <div className="mt-2 text-xs text-[var(--text-secondary)]">{subtitle}</div>}
      </div>
      {trailing && <div className="shrink-0">{trailing}</div>}
    </div>
  )
}
