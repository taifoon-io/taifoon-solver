import { ReactNode } from 'react'
import { cn } from './cn'

interface StatTileProps {
  label: string
  value: string | number | ReactNode
  /** Small value-tone to color the number. */
  tone?: 'default' | 'success' | 'warning' | 'danger' | 'cyan' | 'violet'
  /** Trend indicator: positive number, negative number, or undefined to hide. */
  delta?: number
  unit?: string
  className?: string
}

const toneColor = {
  default: 'text-[var(--text-primary)]',
  success: 'text-[var(--success)]',
  warning: 'text-[var(--warning)]',
  danger: 'text-[var(--danger)]',
  cyan: 'text-[var(--brand-cyan)]',
  violet: 'text-[var(--brand-violet)]',
}

export function StatTile({ label, value, tone = 'default', delta, unit, className }: StatTileProps) {
  return (
    <div
      className={cn(
        'rounded-[var(--r-md)] border border-[var(--border-default)] bg-[var(--bg-elevated)]',
        'px-4 py-3 flex flex-col gap-1 transition-colors hover:border-[var(--border-strong)]',
        className,
      )}
    >
      <span className="text-[10px] font-medium uppercase tracking-[0.16em] text-[var(--text-tertiary)]">
        {label}
      </span>
      <div className="flex items-baseline gap-1">
        <span className={cn('font-mono font-bold text-xl tabular-nums', toneColor[tone])}>
          {value}
        </span>
        {unit && <span className="text-[11px] text-[var(--text-tertiary)] font-mono">{unit}</span>}
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
