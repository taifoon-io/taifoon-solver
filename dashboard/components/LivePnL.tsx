'use client'

import { useEffect, useMemo, useState } from 'react'
import { Card, Tag } from '@/components/ui'
import { protocolColors } from '@/lib/tokens'
import { chainName } from '@/hooks/useSolverEvents'

interface PnlSummary {
  realized_usd_total: number
  fills_total: number
  last_24h_count: number
  by_protocol: Record<string, { fills: number; realized_usd: number; avg_profit_usd: number }>
}

interface OutcomeRecord {
  ts: string
  intent_id: string
  protocol: string
  src_chain: number
  dst_chain: number
  decision: string
  tx_hash: string | null
  explorer_url: string | null
  predicted_profit_usd: number | null
  actual_profit_usd: number | null
  skip_reason: string | null
  error: string | null
}

const POLL_INTERVAL_MS = 3000
const SOLVER_API_BASE =
  (typeof process !== 'undefined' && process.env?.NEXT_PUBLIC_SOLVER_API_URL) || ''

function colorFor(proto: string): string {
  const key = proto.toLowerCase()
  for (const [k, v] of Object.entries(protocolColors)) {
    if (key.includes(k)) return v
  }
  return '#6B7E8F'
}

function fmtUsd(n: number): string {
  const abs = Math.abs(n)
  if (abs >= 1000) return `$${(n / 1000).toFixed(2)}k`
  return `$${n.toFixed(2)}`
}

function fmtAge(ts: string): string {
  const ms = Date.now() - new Date(ts).getTime()
  if (ms < 0) return 'now'
  const s = Math.floor(ms / 1000)
  if (s < 60) return `${s}s ago`
  const m = Math.floor(s / 60)
  if (m < 60) return `${m}m ago`
  const h = Math.floor(m / 60)
  if (h < 24) return `${h}h ago`
  return `${Math.floor(h / 24)}d ago`
}

/** True for any decision that represents a successful on-chain fill. */
function isExecuted(decision: string): boolean {
  return decision === 'executed' || decision === 'confirmed' || decision === 'execute'
}

/**
 * Build a block-explorer URL for a tx_hash on a given chain.
 * Mirrors the server-side `explorer_url_for` so we can derive links
 * client-side when the server field is absent.
 */
function explorerUrl(chainId: number, txHash: string): string | null {
  const bases: Record<number, string> = {
    1: 'https://etherscan.io/tx/',
    10: 'https://optimistic.etherscan.io/tx/',
    137: 'https://polygonscan.com/tx/',
    8453: 'https://basescan.org/tx/',
    42161: 'https://arbiscan.io/tx/',
    59144: 'https://lineascan.build/tx/',
    324: 'https://explorer.zksync.io/tx/',
    56: 'https://bscscan.com/tx/',
    43114: 'https://snowtrace.io/tx/',
  }
  const base = bases[chainId]
  return base ? `${base}${txHash}` : null
}

function decisionPill(decision: string) {
  if (isExecuted(decision)) {
    return (
      <span className="inline-flex items-center gap-1 text-[10px] font-mono px-1.5 py-0.5 rounded bg-[rgba(20,241,149,0.1)] text-[var(--success)] font-semibold tracking-wide">
        <span className="w-1.5 h-1.5 rounded-full bg-[var(--success)]" />
        FILLED
      </span>
    )
  }
  if (decision === 'reverted') {
    return (
      <span className="text-[10px] font-mono px-1.5 py-0.5 rounded bg-[rgba(255,107,107,0.1)] text-[var(--danger)]">
        REVERTED
      </span>
    )
  }
  if (decision === 'dry_run') {
    return (
      <span className="text-[10px] font-mono px-1.5 py-0.5 rounded bg-[rgba(255,180,84,0.1)] text-[var(--warning)]">
        DRY RUN
      </span>
    )
  }
  if (decision.startsWith('skip')) {
    return (
      <span className="text-[10px] font-mono px-1.5 py-0.5 rounded bg-[var(--bg-raised)] text-[var(--text-tertiary)]">
        SKIPPED
      </span>
    )
  }
  return (
    <span className="text-[10px] font-mono px-1.5 py-0.5 rounded bg-[var(--bg-raised)] text-[var(--text-tertiary)]">
      {decision.toUpperCase()}
    </span>
  )
}

interface OutcomeRowProps {
  o: OutcomeRecord
  highlight?: boolean
}

function OutcomeRow({ o, highlight }: OutcomeRowProps) {
  // Prefer the server-computed explorer_url; fall back to client-side derivation
  // using the destination chain (where the fill tx lands).
  const txUrl =
    o.explorer_url ??
    (o.tx_hash ? explorerUrl(o.dst_chain, o.tx_hash) : null)

  return (
    <div
      className={`flex items-center gap-2 py-2 text-[11px] flex-wrap ${
        highlight
          ? 'bg-[rgba(20,241,149,0.04)] rounded px-2 -mx-2 border border-[rgba(20,241,149,0.12)]'
          : ''
      }`}
    >
      <span
        className="w-1.5 h-1.5 rounded-full shrink-0"
        style={{ background: colorFor(o.protocol) }}
      />
      <span
        className="font-mono uppercase tracking-[0.08em] shrink-0"
        style={{ color: colorFor(o.protocol) }}
      >
        {o.protocol.replace(/_v\d+$/, '').replace(/_/g, ' ')}
      </span>
      <span className="text-[var(--text-tertiary)] font-mono shrink-0">
        {chainName(o.src_chain)} &rarr; {chainName(o.dst_chain)}
      </span>
      {decisionPill(o.decision)}
      <span className="ml-auto font-mono tabular-nums">
        {o.actual_profit_usd != null ? (
          <span style={{ color: o.actual_profit_usd >= 0 ? 'var(--success)' : 'var(--danger)' }}>
            {o.actual_profit_usd >= 0 ? '+' : ''}{fmtUsd(o.actual_profit_usd)}
          </span>
        ) : (
          <span className="text-[var(--text-disabled)]">—</span>
        )}
      </span>
      {txUrl && o.tx_hash ? (
        <a
          href={txUrl}
          target="_blank"
          rel="noreferrer"
          className="font-mono text-[var(--brand-blue)] hover:underline shrink-0"
          title={o.tx_hash}
        >
          {o.tx_hash.slice(0, 8)}&hellip;&#8599;
        </a>
      ) : (
        <span className="font-mono text-[var(--text-disabled)] shrink-0">no tx</span>
      )}
      <span className="text-[var(--text-tertiary)] shrink-0">{fmtAge(o.ts)}</span>
    </div>
  )
}

export default function LivePnL() {
  const [summary, setSummary] = useState<PnlSummary | null>(null)
  const [outcomes, setOutcomes] = useState<OutcomeRecord[]>([])
  const [error, setError] = useState<string | null>(null)
  const [othersOpen, setOthersOpen] = useState(false)

  useEffect(() => {
    let cancelled = false
    const tick = async () => {
      try {
        const [pnlRes, execRes, otherRes] = await Promise.all([
          fetch(`${SOLVER_API_BASE}/api/solver/pnl`, { cache: 'no-store' }),
          fetch(`${SOLVER_API_BASE}/api/solver/outcomes?limit=100&decision=executed`, { cache: 'no-store' }),
          fetch(`${SOLVER_API_BASE}/api/solver/outcomes?limit=20`, { cache: 'no-store' }),
        ])
        if (!pnlRes.ok) throw new Error(`pnl HTTP ${pnlRes.status}`)
        const pnl: PnlSummary = await pnlRes.json()
        const executed: OutcomeRecord[] = execRes.ok ? await execRes.json() : []
        const recent: OutcomeRecord[] = otherRes.ok ? await otherRes.json() : []
        // Merge: executed fills + recent non-executed outcomes (dedup by intent_id)
        const execIds = new Set(executed.map((r) => r.intent_id))
        const others = recent.filter((r) => !isExecuted(r.decision) && !execIds.has(r.intent_id))
        if (!cancelled) {
          setSummary(pnl)
          setOutcomes([...executed, ...others])
          setError(null)
        }
      } catch (e: unknown) {
        if (!cancelled) setError(e instanceof Error ? e.message : 'fetch failed')
      }
    }
    tick()
    const id = setInterval(tick, POLL_INTERVAL_MS)
    return () => {
      cancelled = true
      clearInterval(id)
    }
  }, [])

  // Split: executed fills shown first (highlighted), then up to 8 others collapsed.
  const { executedFills, otherOutcomes } = useMemo(() => {
    const executed = outcomes.filter((o) => isExecuted(o.decision))
    const others = outcomes.filter((o) => !isExecuted(o.decision)).slice(0, 8)
    return { executedFills: executed, otherOutcomes: others }
  }, [outcomes])

  const protocols = useMemo(() => {
    if (!summary) return [] as { name: string; fills: number; realized: number; pct: number; color: string }[]
    const total = Math.max(summary.realized_usd_total, 0.0001)
    return Object.entries(summary.by_protocol)
      .map(([name, p]) => ({
        name,
        fills: p.fills,
        realized: p.realized_usd,
        pct: (p.realized_usd / total) * 100,
        color: colorFor(name),
      }))
      .sort((a, b) => b.realized - a.realized)
  }, [summary])

  const isPositive = (summary?.realized_usd_total ?? 0) >= 0

  return (
    <Card padding="none" aria-label="Live P&L">
      {/* Header */}
      <div className="flex items-center justify-between px-4 py-3 border-b border-[var(--border-subtle)]">
        <div className="flex items-center gap-2">
          <Tag>Live P&amp;L</Tag>
          {!error && (
            <span className="w-1.5 h-1.5 rounded-full bg-[var(--solana-mint)] pulse-live" />
          )}
        </div>
        {error ? (
          <span className="text-[10px] font-mono text-[var(--danger)]">solver-api: {error}</span>
        ) : (
          <span className="text-[10px] font-mono text-[var(--text-tertiary)] tracking-[0.12em]">
            refreshes every 3s
          </span>
        )}
      </div>

      <div className="p-4 space-y-5">
        {/* KPI row */}
        <div className="grid grid-cols-3 gap-3">
          <KpiTile
            label="REALIZED"
            value={summary ? fmtUsd(summary.realized_usd_total) : '—'}
            tone={isPositive ? 'mint' : 'danger'}
          />
          <KpiTile label="TOTAL FILLS" value={summary ? String(summary.fills_total) : '—'} tone="blue" />
          <KpiTile label="LAST 24H" value={summary ? String(summary.last_24h_count) : '—'} />
        </div>

        {/* Protocol stacked bar */}
        {protocols.length > 0 && (
          <div>
            <div className="h-1.5 w-full overflow-hidden rounded-full bg-[var(--bg-raised)] flex mb-3">
              {protocols.map((p) => (
                <div
                  key={p.name}
                  style={{ width: `${p.pct}%`, background: p.color, transition: 'width 600ms var(--ease-out)' }}
                  title={`${p.name}: ${fmtUsd(p.realized)} (${p.fills} fills)`}
                />
              ))}
            </div>
            <ul className="flex flex-wrap gap-x-4 gap-y-1.5">
              {protocols.map((p) => (
                <li key={p.name} className="flex items-center gap-1.5 text-[11px]">
                  <span className="w-2 h-2 rounded-full shrink-0" style={{ background: p.color }} />
                  <span className="font-mono uppercase tracking-[0.1em] text-[var(--text-secondary)]">
                    {p.name.replace(/_/g, ' ')}
                  </span>
                  <span className="font-mono text-[var(--text-tertiary)]">
                    {fmtUsd(p.realized)} · {p.fills}
                  </span>
                </li>
              ))}
            </ul>
          </div>
        )}

        {/* Confirmed / executed fills — always shown prominently at top */}
        <div>
          <div className="text-[10px] tracking-[0.24em] uppercase text-[var(--text-tertiary)] mb-2">
            Confirmed fills
          </div>
          {executedFills.length === 0 ? (
            <div className="text-[var(--text-tertiary)] text-[12px] font-mono text-center py-6 bg-[var(--bg-raised)] rounded">
              No confirmed fills yet — start broadcasting and they&apos;ll appear here.
            </div>
          ) : (
            <div className="space-y-1">
              {executedFills.map((o) => (
                <OutcomeRow key={o.intent_id} o={o} highlight />
              ))}
            </div>
          )}
        </div>

        {/* Other recent outcomes — collapsible */}
        {otherOutcomes.length > 0 && (
          <div>
            <button
              type="button"
              onClick={() => setOthersOpen((v) => !v)}
              className="flex items-center gap-1.5 text-[10px] tracking-[0.24em] uppercase text-[var(--text-tertiary)] hover:text-[var(--text-secondary)] transition-colors mb-2 w-full text-left"
            >
              <span>{othersOpen ? '▾' : '▸'}</span>
              <span>Other recent decisions ({otherOutcomes.length})</span>
            </button>
            {othersOpen && (
              <div className="divide-y divide-[var(--border-subtle)]">
                {otherOutcomes.map((o) => (
                  <OutcomeRow key={o.intent_id} o={o} />
                ))}
              </div>
            )}
          </div>
        )}
      </div>
    </Card>
  )
}

function KpiTile({
  label,
  value,
  tone = 'default',
}: {
  label: string
  value: string
  tone?: 'default' | 'mint' | 'danger' | 'blue'
}) {
  const color = {
    default: 'var(--text-primary)',
    mint: 'var(--solana-mint)',
    danger: 'var(--danger)',
    blue: 'var(--brand-blue)',
  }[tone]
  return (
    <div className="bg-[var(--bg-raised)] rounded-[var(--r-md)] px-3 py-2.5">
      <div className="text-[9px] tracking-[0.24em] uppercase text-[var(--text-tertiary)]">{label}</div>
      <div className="font-mono text-[22px] tabular-nums mt-1" style={{ color }}>{value}</div>
    </div>
  )
}
