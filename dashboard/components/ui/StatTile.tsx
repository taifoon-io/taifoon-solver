import { ReactNode } from 'react'
import { cn } from './cn'

interface StatTileProps {
  /** Small mono prefix label, e.g. "real-time". */
  label: string
  value: string | number | ReactNode
  tone?: 'default' | 'success' | 'warning' | 'danger' | 'blue' | 'mint' | 'violet'
  delta?: number
  unit?: string
  className?: string
  /** Layout: 'inline' = label and value on one line (taifoon.io style), 'stack' = label above value. */
  layout?: 'inline' | 'stack'
}

const toneColor = {
  default: 'text-[var(--text-primary)]',
  success: 'text-[var(--success)]',
  warning: 'text-[var(--warning)]',
  danger: 'text-[var(--danger)]',
  blue: 'text-[var(--brand-blue)]',
  mint: 'text-[var(--solana-mint)]',
  violet: 'text-[var(--solana-violet)]',
}

/**
 * Stat callout — taifoon.io style. No card chrome by default; a tiny
 * mono prefix sits inline before a larger mono number.
 *
 *   real-time  41 chains
 *   median     127 ms
 */
export function StatTile({
  label,
  value,
  tone = 'default',
  delta,
  unit,
  className,
  layout = 'stack',
}: StatTileProps) {
  if (layout === 'inline') {
    return (
      <div className={cn('flex items-baseline gap-2', className)}>
        <span className="tf-stat-prefix">{label}</span>
        <span className={cn('tf-stat-value', toneColor[tone])}>
          {value}
          {unit && <span className="text-[14px] text-[var(--text-tertiary)] ml-1">{unit}</span>}
        </span>
      </div>
    )
  }
  return (
    <div className={cn('flex flex-col gap-1.5', className)}>
      <span className="tf-stat-prefix uppercase tracking-[0.2em]">{label}</span>
      <div className="flex items-baseline gap-1.5">
        <span className={cn('tf-stat-value', toneColor[tone])}>{value}</span>
        {unit && <span className="text-[12px] text-[var(--text-tertiary)] font-mono">{unit}</span>}
      </div>
      {delta !== undefined && (
        <span
          className={cn(
            'text-[10px] font-mono',
            delta >= 0 ? 'text-[var(--success)]' : 'text-[var(--danger)]',
          )}
        >
          {delta >= 0 ? '↑' : '↓'} {Math.abs(delta).toFixed(2)}%
        </span>
      )}
    </div>
  )
}
