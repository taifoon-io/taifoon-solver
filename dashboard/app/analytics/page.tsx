'use client'

import { useCallback, useEffect, useMemo, useState } from 'react'
import { NavBar, Footer, Card, Tag, StatTile } from '@/components/ui'
import { protocolColors } from '@/lib/tokens'

// ── Types ─────────────────────────────────────────────────────────────────────

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
  actual_profit_usd: number | null
}

interface StatsSummary {
  total_intents?: number
  executed_fills?: number
  failed_fills?: number
  success_rate?: number
  latency_ms?: number
  net_profit_today_usd?: number
}

const SOLVER_API_BASE =
  (typeof process !== 'undefined' && process.env?.NEXT_PUBLIC_SOLVER_API_URL) || ''

const CHAIN_NAMES: Record<number, string> = {
  1: 'ETH', 10: 'OP', 56: 'BSC', 137: 'POL', 8453: 'BASE',
  42161: 'ARB', 43114: 'AVAX', 59144: 'LIN', 534352: 'SCR',
}
function chainName(id: number): string {
  return CHAIN_NAMES[id] ?? `c${id}`
}

function fmtUsd(n: number, compact = false): string {
  const abs = Math.abs(n)
  if (compact) {
    if (abs >= 1000) return `$${(n / 1000).toFixed(1)}k`
    return `$${n.toFixed(2)}`
  }
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

function colorFor(proto: string): string {
  const key = proto.toLowerCase()
  for (const [k, v] of Object.entries(protocolColors)) {
    if (key.includes(k)) return v
  }
  return '#6B7E8F'
}

// ── Fill rate sparkline ────────────────────────────────────────────────────────

const SPARKLINE_BINS = 20
const SPARKLINE_W = 240
const SPARKLINE_H = 48

function FillRateSparkline({ outcomes }: { outcomes: OutcomeRecord[] }) {
  const bins = useMemo(() => {
    if (outcomes.length === 0) return new Array(SPARKLINE_BINS).fill(0) as number[]
    const now = Date.now()
    const span = Math.max(
      now - new Date(outcomes[outcomes.length - 1]?.ts ?? now).getTime(),
      60_000,
    )
    const binMs = span / SPARKLINE_BINS
    const arr = new Array(SPARKLINE_BINS).fill(0) as number[]
    for (const o of outcomes) {
      const age = now - new Date(o.ts).getTime()
      const idx = Math.min(SPARKLINE_BINS - 1, Math.floor((span - age) / binMs))
      if (idx >= 0) arr[idx]++
    }
    return arr
  }, [outcomes])

  const max = Math.max(...bins, 1)
  const pts = bins
    .map((v, i) => {
      const x = (i / (SPARKLINE_BINS - 1)) * SPARKLINE_W
      const y = SPARKLINE_H - (v / max) * (SPARKLINE_H - 4) - 2
      return `${x.toFixed(1)},${y.toFixed(1)}`
    })
    .join(' ')

  return (
    <svg
      viewBox={`0 0 ${SPARKLINE_W} ${SPARKLINE_H}`}
      preserveAspectRatio="none"
      className="w-full"
      style={{ height: SPARKLINE_H }}
    >
      <defs>
        <linearGradient id="spark-grad" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0%" stopColor="#3DA5FF" stopOpacity="0.4" />
          <stop offset="100%" stopColor="#3DA5FF" stopOpacity="0.02" />
        </linearGradient>
      </defs>
      <polyline
        points={pts}
        fill="none"
        stroke="#3DA5FF"
        strokeWidth="1.5"
        strokeLinejoin="round"
      />
      <polygon
        points={`0,${SPARKLINE_H} ${pts} ${SPARKLINE_W},${SPARKLINE_H}`}
        fill="url(#spark-grad)"
      />
    </svg>
  )
}

// ── Protocol card ──────────────────────────────────────────────────────────────

function ProtocolCard({
  name,
  fills,
  realized,
  avg,
  shareOfTotal,
  outcomes,
}: {
  name: string
  fills: number
  realized: number
  avg: number
  shareOfTotal: number
  outcomes: OutcomeRecord[]
}) {
  const color = colorFor(name)
  const protoOutcomes = outcomes.filter((o) =>
    o.protocol.toLowerCase().includes(name.split('_')[0]),
  )
  const successRate =
    protoOutcomes.length > 0
      ? (protoOutcomes.filter((o) => o.decision === 'confirmed' || o.decision === 'executed').length /
          protoOutcomes.length) *
        100
      : 0

  return (
    <Card padding="md" className="flex flex-col gap-3">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <span
            className="w-2 h-2 rounded-full"
            style={{ background: color }}
          />
          <span
            className="font-mono text-[12px] tracking-[0.1em] uppercase"
            style={{ color }}
          >
            {name.replace(/_v\d+$/, '').replace(/_/g, ' ')}
          </span>
        </div>
        <span className="font-mono text-[10px] text-[var(--text-tertiary)]">
          {shareOfTotal.toFixed(0)}% of P&amp;L
        </span>
      </div>

      {/* Share bar */}
      <div className="h-1 bg-[var(--bg-raised)] rounded-full overflow-hidden">
        <div
          className="h-full rounded-full transition-all duration-700"
          style={{ width: `${shareOfTotal}%`, background: color }}
        />
      </div>

      <div className="grid grid-cols-3 gap-2 pt-1">
        <Micro
          label="Realized"
          value={fmtUsd(realized)}
          color={realized >= 0 ? 'var(--success)' : 'var(--danger)'}
        />
        <Micro label="Fills" value={String(fills)} />
        <Micro label="Avg / fill" value={fmtUsd(avg)} />
      </div>

      <div className="flex items-center justify-between text-[10px] font-mono border-t border-[var(--border-subtle)] pt-2">
        <span className="text-[var(--text-tertiary)]">success rate</span>
        <span style={{ color: successRate > 80 ? 'var(--success)' : successRate > 50 ? 'var(--warning)' : 'var(--danger)' }}>
          {fills > 0 ? `${successRate.toFixed(0)}%` : '—'}
        </span>
      </div>
    </Card>
  )
}

function Micro({ label, value, color }: { label: string; value: string; color?: string }) {
  return (
    <div className="flex flex-col gap-0.5">
      <span className="text-[9px] tracking-[0.2em] uppercase text-[var(--text-tertiary)]">{label}</span>
      <span className="font-mono text-[13px] tabular-nums" style={{ color: color ?? 'var(--text-primary)' }}>
        {value}
      </span>
    </div>
  )
}

// ── Chain flow matrix ─────────────────────────────────────────────────────────

function ChainFlowMatrix({ outcomes }: { outcomes: OutcomeRecord[] }) {
  const flows = useMemo(() => {
    const map: Record<string, { count: number; profit: number }> = {}
    for (const o of outcomes) {
      if (o.decision !== 'confirmed' && o.decision !== 'executed') continue
      const key = `${o.src_chain}→${o.dst_chain}`
      const entry = map[key] ?? { count: 0, profit: 0 }
      entry.count++
      entry.profit += o.actual_profit_usd ?? 0
      map[key] = entry
    }
    return Object.entries(map)
      .map(([key, v]) => {
        const [src, dst] = key.split('→').map(Number)
        return { src, dst, ...v }
      })
      .sort((a, b) => b.count - a.count)
      .slice(0, 12)
  }, [outcomes])

  return (
    <Card padding="none">
      <div className="flex items-center justify-between px-4 py-3 border-b border-[var(--border-subtle)]">
        <Tag>Chain Flows</Tag>
        <span className="font-mono text-[10px] text-[var(--text-tertiary)] tracking-[0.12em]">
          top {flows.length} routes · confirmed only
        </span>
      </div>
      <div className="divide-y divide-[var(--border-subtle)]">
        {flows.length === 0 && (
          <div className="text-[var(--text-tertiary)] text-xs text-center py-8 font-mono">
            No confirmed fills yet
          </div>
        )}
        {flows.map((f) => (
          <div
            key={`${f.src}→${f.dst}`}
            className="flex items-center justify-between px-4 py-2.5 text-[12px]"
          >
            <div className="flex items-center gap-2 font-mono">
              <span className="text-[var(--text-primary)]">{chainName(f.src)}</span>
              <span className="text-[var(--text-tertiary)]">→</span>
              <span className="text-[var(--text-primary)]">{chainName(f.dst)}</span>
            </div>
            <div className="flex items-center gap-4 font-mono">
              <span className="text-[var(--text-tertiary)]">{f.count} fills</span>
              <span style={{ color: f.profit >= 0 ? 'var(--success)' : 'var(--danger)' }}>
                {f.profit >= 0 ? '+' : ''}{fmtUsd(f.profit)}
              </span>
            </div>
          </div>
        ))}
      </div>
    </Card>
  )
}

// ── Recent fills table ────────────────────────────────────────────────────────

function RecentFillsTable({ outcomes }: { outcomes: OutcomeRecord[] }) {
  const [filter, setFilter] = useState<'all' | 'confirmed' | 'skipped' | 'reverted'>('all')

  const filtered = useMemo(() => {
    if (filter === 'all') return outcomes
    if (filter === 'confirmed') return outcomes.filter(o => o.decision === 'confirmed' || o.decision === 'executed')
    if (filter === 'skipped') return outcomes.filter(o => o.decision.startsWith('skip') || o.decision === 'dry_run')
    return outcomes.filter(o => o.decision === 'reverted')
  }, [outcomes, filter])

  const filters = ['all', 'confirmed', 'skipped', 'reverted'] as const

  return (
    <Card padding="none">
      <div className="flex items-center justify-between px-4 py-3 border-b border-[var(--border-subtle)] flex-wrap gap-2">
        <Tag>All Fills</Tag>
        <div className="flex items-center gap-4">
          {filters.map((f) => (
            <button
              key={f}
              onClick={() => setFilter(f)}
              className={`font-mono text-[10px] tracking-[0.18em] uppercase transition-colors ${
                filter === f
                  ? 'text-[var(--brand-blue)]'
                  : 'text-[var(--text-tertiary)] hover:text-[var(--text-primary)]'
              }`}
            >
              {filter === f ? `[ ${f} ]` : f}
            </button>
          ))}
        </div>
      </div>
      <div className="divide-y divide-[var(--border-subtle)] max-h-[480px] overflow-y-auto">
        {filtered.length === 0 && (
          <div className="text-[var(--text-tertiary)] text-xs text-center py-10 font-mono">
            No fills match this filter
          </div>
        )}
        {filtered.slice(0, 100).map((o) => {
          const isOk = o.decision === 'confirmed' || o.decision === 'executed'
          const isRev = o.decision === 'reverted'
          return (
            <div
              key={o.intent_id}
              className={`flex items-center gap-2 px-4 py-2.5 text-[11px] flex-wrap transition-colors hover:bg-[var(--bg-raised)] ${
                isOk ? 'border-l-2 border-[var(--success)]' : isRev ? 'border-l-2 border-[var(--danger)]' : ''
              }`}
            >
              <span
                className="font-mono uppercase tracking-[0.08em] shrink-0 w-[80px] truncate"
                style={{ color: colorFor(o.protocol) }}
              >
                {o.protocol.replace(/_v\d+$/, '').replace(/_/g, ' ')}
              </span>
              <span className="font-mono text-[var(--text-tertiary)] shrink-0">
                {chainName(o.src_chain)}→{chainName(o.dst_chain)}
              </span>
              <span
                className="text-[10px] font-mono px-1.5 py-0.5 rounded shrink-0"
                style={{
                  color: isOk ? 'var(--success)' : isRev ? 'var(--danger)' : 'var(--text-tertiary)',
                  background: isOk ? 'rgba(20,241,149,0.08)' : isRev ? 'rgba(255,107,107,0.08)' : 'var(--bg-raised)',
                }}
              >
                {o.decision.toUpperCase()}
              </span>
              <span className="ml-auto font-mono tabular-nums shrink-0">
                {o.actual_profit_usd != null ? (
                  <span style={{ color: o.actual_profit_usd >= 0 ? 'var(--success)' : 'var(--danger)' }}>
                    {o.actual_profit_usd >= 0 ? '+' : ''}{fmtUsd(o.actual_profit_usd)}
                  </span>
                ) : (
                  <span className="text-[var(--text-disabled)]">—</span>
                )}
              </span>
              <span className="text-[var(--text-tertiary)] shrink-0">{fmtAge(o.ts)}</span>
            </div>
          )
        })}
      </div>
    </Card>
  )
}

// ── Main page ─────────────────────────────────────────────────────────────────

export default function AnalyticsPage() {
  const [pnl, setPnl] = useState<PnlSummary | null>(null)
  const [outcomes, setOutcomes] = useState<OutcomeRecord[]>([])
  const [stats, setStats] = useState<StatsSummary | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [lastRefresh, setLastRefresh] = useState<Date | null>(null)

  const refresh = useCallback(async () => {
    try {
      const [pnlRes, outRes, statsRes] = await Promise.allSettled([
        fetch(`${SOLVER_API_BASE}/api/solver/pnl`, { cache: 'no-store' }),
        fetch(`${SOLVER_API_BASE}/api/solver/outcomes?limit=200`, { cache: 'no-store' }),
        fetch(`${SOLVER_API_BASE}/api/solver/stats`, { cache: 'no-store' }),
      ])
      if (pnlRes.status === 'fulfilled' && pnlRes.value.ok) setPnl(await pnlRes.value.json())
      if (outRes.status === 'fulfilled' && outRes.value.ok) setOutcomes(await outRes.value.json())
      if (statsRes.status === 'fulfilled' && statsRes.value.ok) setStats(await statsRes.value.json())
      setError(null)
      setLastRefresh(new Date())
    } catch (e) {
      setError(e instanceof Error ? e.message : 'fetch failed')
    }
  }, [])

  useEffect(() => {
    refresh()
    const id = setInterval(refresh, 10_000)
    return () => clearInterval(id)
  }, [refresh])

  // Per-protocol stats
  const protocols = useMemo(() => {
    if (!pnl) return []
    const total = Math.max(pnl.realized_usd_total, 0.0001)
    return Object.entries(pnl.by_protocol)
      .map(([name, p]) => ({
        name,
        fills: p.fills,
        realized: p.realized_usd,
        avg: p.avg_profit_usd,
        shareOfTotal: (p.realized_usd / total) * 100,
      }))
      .sort((a, b) => b.realized - a.realized)
  }, [pnl])

  const confirmedOutcomes = outcomes.filter(
    (o) => o.decision === 'confirmed' || o.decision === 'executed',
  )

  const totalRealized = pnl?.realized_usd_total ?? 0
  const successPct =
    stats?.success_rate != null
      ? (stats.success_rate * 100).toFixed(0) + '%'
      : confirmedOutcomes.length > 0 && outcomes.length > 0
        ? ((confirmedOutcomes.length / outcomes.length) * 100).toFixed(0) + '%'
        : '—'

  return (
    <>
      <NavBar />
      <main className="flex-1">
        {/* Header */}
        <div className="border-b border-[var(--border-subtle)]">
          <div className="max-w-[1400px] mx-auto px-6 py-10 flex items-end justify-between flex-wrap gap-6">
            <div>
              <Tag>Analytics</Tag>
              <h1 className="tf-display tf-gradient-silver mt-4 text-[clamp(1.8rem,3.5vw,2.8rem)]">
                Fill performance.
              </h1>
              <p className="mt-2 text-sm text-[var(--text-secondary)] max-w-[520px] leading-relaxed">
                Per-protocol P&amp;L, chain flow matrix, and fill rate trends.
                Pulls from the live solver outcome log — refreshes every 10s.
              </p>
            </div>
            <div className="flex items-center gap-3">
              {error && (
                <span className="font-mono text-[11px] text-[var(--danger)]">
                  {error}
                </span>
              )}
              {lastRefresh && (
                <span className="font-mono text-[11px] text-[var(--text-tertiary)]">
                  updated {fmtAge(lastRefresh.toISOString())}
                </span>
              )}
              <button
                onClick={refresh}
                className="font-mono text-[12px] tracking-[0.16em] text-[var(--brand-blue)] border border-[var(--brand-blue)]/40 hover:border-[var(--brand-blue)] hover:bg-[var(--brand-blue)]/10 px-3 h-8 inline-flex items-center rounded-[var(--r-sm)] transition-all"
              >
                REFRESH
              </button>
            </div>
          </div>

          {/* KPI strip */}
          <div className="max-w-[1400px] mx-auto px-6 pb-8 grid grid-cols-2 sm:grid-cols-5 gap-x-10 gap-y-4">
            <StatTile
              label="REALIZED P&L"
              value={fmtUsd(totalRealized)}
              tone={totalRealized >= 0 ? 'mint' : 'danger'}
            />
            <StatTile label="FILLS" value={pnl?.fills_total ?? '—'} tone="blue" />
            <StatTile label="LAST 24H" value={pnl?.last_24h_count ?? '—'} />
            <StatTile label="SUCCESS RATE" value={successPct} tone="blue" />
            <StatTile label="LATENCY" value={stats?.latency_ms != null ? `${stats.latency_ms}ms` : '—'} />
          </div>
        </div>

        {/* Fill rate sparkline */}
        <section className="max-w-[1400px] mx-auto px-6 py-6">
          <Card padding="md">
            <div className="flex items-center justify-between mb-3">
              <Tag>Fill Rate</Tag>
              <span className="font-mono text-[10px] text-[var(--text-tertiary)] tracking-[0.12em]">
                {outcomes.length} outcomes · binned over session
              </span>
            </div>
            {outcomes.length > 0 ? (
              <FillRateSparkline outcomes={outcomes} />
            ) : (
              <div className="h-12 flex items-center justify-center text-[var(--text-tertiary)] text-xs font-mono">
                No outcome data yet
              </div>
            )}
          </Card>
        </section>

        {/* Protocol breakdown */}
        {protocols.length > 0 && (
          <section className="max-w-[1400px] mx-auto px-6 pb-6">
            <div className="flex items-center gap-3 mb-4">
              <Tag>Protocol Breakdown</Tag>
              <span className="font-mono text-[10px] text-[var(--text-tertiary)] tracking-[0.12em]">
                {protocols.length} active
              </span>
            </div>
            <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-3">
              {protocols.map((p) => (
                <ProtocolCard
                  key={p.name}
                  outcomes={outcomes}
                  {...p}
                />
              ))}
            </div>
          </section>
        )}

        {protocols.length === 0 && !error && (
          <section className="max-w-[1400px] mx-auto px-6 pb-6">
            <Card padding="lg" className="text-center">
              <div className="text-[var(--text-tertiary)] text-sm font-mono py-6">
                No fills recorded yet — protocol breakdown will appear here once the solver executes fills.
              </div>
            </Card>
          </section>
        )}

        {/* Chain flow + recent fills */}
        <div className="max-w-[1400px] mx-auto px-6 pb-6 grid grid-cols-1 lg:grid-cols-2 gap-3">
          <ChainFlowMatrix outcomes={outcomes} />
          <Card padding="none">
            <div className="px-4 py-3 border-b border-[var(--border-subtle)]">
              <Tag>P&amp;L distribution</Tag>
            </div>
            <div className="p-4">
              <PnlHistogram outcomes={outcomes} />
            </div>
          </Card>
        </div>

        <section className="max-w-[1400px] mx-auto px-6 pb-16">
          <RecentFillsTable outcomes={outcomes} />
        </section>
      </main>
      <Footer />
    </>
  )
}

// ── P&L histogram ─────────────────────────────────────────────────────────────

function PnlHistogram({ outcomes }: { outcomes: OutcomeRecord[] }) {
  const profits = outcomes
    .filter((o) => o.actual_profit_usd != null)
    .map((o) => o.actual_profit_usd as number)

  if (profits.length === 0) {
    return (
      <div className="h-32 flex items-center justify-center text-[var(--text-tertiary)] text-xs font-mono">
        No profit data yet
      </div>
    )
  }

  const BINS = 16
  const min = Math.min(...profits)
  const max = Math.max(...profits)
  const range = max - min || 1
  const step = range / BINS

  const counts = new Array(BINS).fill(0) as number[]
  for (const p of profits) {
    const idx = Math.min(BINS - 1, Math.floor((p - min) / step))
    counts[idx]++
  }

  const peak = Math.max(...counts, 1)
  const H = 80
  const W = 220

  return (
    <div>
      <svg viewBox={`0 0 ${W} ${H}`} className="w-full" style={{ height: H }}>
        {counts.map((c, i) => {
          const barW = W / BINS - 1
          const barH = (c / peak) * (H - 8)
          const x = i * (W / BINS)
          const y = H - barH
          const midVal = min + step * (i + 0.5)
          const color = midVal >= 0 ? '#14F195' : '#FF6B6B'
          return (
            <rect
              key={i}
              x={x}
              y={y}
              width={barW}
              height={barH}
              fill={color}
              fillOpacity="0.7"
              rx="1"
            />
          )
        })}
        <line x1={W / 2} x2={W / 2} y1="0" y2={H} stroke="rgba(230,240,247,0.15)" strokeWidth="1" strokeDasharray="3,3" />
      </svg>
      <div className="flex justify-between font-mono text-[9px] text-[var(--text-disabled)] mt-1">
        <span>{fmtUsd(min, true)}</span>
        <span>0</span>
        <span>{fmtUsd(max, true)}</span>
      </div>
    </div>
  )
}
