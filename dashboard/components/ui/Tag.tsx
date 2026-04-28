/**
 * Tag — taifoon.io's signature [ BRACKETED LABEL ] in azure mono.
 *
 *   <Tag>The engine</Tag>   →   [ THE ENGINE ]
 */
export function Tag({
  children,
  tone = 'blue',
  className = '',
}: {
  children: string
  tone?: 'blue' | 'mint' | 'violet' | 'muted'
  className?: string
}) {
  const colorMap = {
    blue: 'text-[var(--brand-blue)]',
    mint: 'text-[var(--solana-mint)]',
    violet: 'text-[var(--solana-violet)]',
    muted: 'text-[var(--text-tertiary)]',
  }
  return (
    <span
      className={`inline-block font-mono text-[12px] uppercase tracking-[0.25em] ${colorMap[tone]} ${className}`}
    >
      [ {children.toUpperCase()} ]
    </span>
  )
}

/**
 * PhaseLabel — "PHASE 01 — OBSERVE" style indicator used on timelines and
 * narrative sections.
 */
export function PhaseLabel({
  phase,
  step,
  tone = 'blue',
}: {
  phase: number
  step: string
  tone?: 'blue' | 'mint' | 'violet'
}) {
  const colorMap = {
    blue: 'text-[var(--brand-blue)]',
    mint: 'text-[var(--solana-mint)]',
    violet: 'text-[var(--solana-violet)]',
  }
  return (
    <span
      className={`font-mono text-[11px] uppercase tracking-[0.25em] ${colorMap[tone]}`}
    >
      PHASE {String(phase).padStart(2, '0')} — {step.toUpperCase()}
    </span>
  )
}
