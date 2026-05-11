'use client'

import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { Card, Tag, Badge } from '@/components/ui'
import { chainName } from '@/hooks/useSolverEvents'

interface PnlByProtocol {
  fills: number
  realized_usd: number
  avg_profit_usd: number
}

interface PnlSummary {
  realized_usd_total: number
  fills_total: number
  last_24h_count: number
  by_protocol: Record<string, PnlByProtocol>
}

interface BridgeAction {
  src_chain: number
  dst_chain: number
  token_symbol: string
  amount_usd: number
  kind: string
  tx_hash?: string | null
  status?: string | null
}

interface ActionLogEntry {
  ts: string
  cycle: number
  action: BridgeAction
}

interface RebalancerStatus {
  last_run_at: string | null
  next_run_at: string | null
  last_actions: ActionLogEntry[]
  blocked_reason: string | null
  interval_secs: number
  cycle: number
}

const POLL_MS = 15_000
const SOLVER_API_BASE =
  (typeof process !== 'undefined' && process.env?.NEXT_PUBLIC_SOLVER_API_URL) || ''
const SOLVER_API_TOKEN =
  (typeof process !== 'undefined' && process.env?.NEXT_PUBLIC_SOLVER_API_TOKEN) || ''

const PROTO_COLORS: Record<string, string> = {
  across_v3: '#3DA5FF',
  debridge_dln: '#FF8A4C',
  mayan_swift: '#9945FF',
}

const PROTO_LABELS: Record<string, string> = {
  across_v3: 'Across V3',
  debridge_dln: 'deBridge DLN',
  mayan_swift: 'Mayan Swift',
}

function protoColor(key: string): string {
  return PROTO_COLORS[key] ?? PROTO_COLORS[key.split('_')[0]] ?? '#6B7E8F'
}

function protoLabel(key: string): string {
  return PROTO_LABELS[key] ?? key.replace(/_/g, ' ').replace(/\b\w/g, (c) => c.toUpperCase())
}

function fmtUsd(n: number): string {
  if (Math.abs(n) >= 1000) return `$${(n / 1000).toFixed(2)}k`
  return `$${n.toFixed(2)}`
}

function fmtAge(ts: string | null): string {
  if (!ts) return '—'
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

function fmtCountdown(nextAt: string | null): string {
  if (!nextAt) return '—'
  const ms = new Date(nextAt).getTime() - Date.now()
  if (ms <= 0) return 'due now'
  const s = Math.floor(ms / 1000)
  if (s < 60) return `in ${s}s`
  return `in ${Math.floor(s / 60)}m`
}

function authHeaders(): HeadersInit {
  return SOLVER_API_TOKEN ? { Authorization: `Bearer ${SOLVER_API_TOKEN}` } : {}
}

// ── Rolling counter ───────────────────────────────────────────────────────────

function RollingNumber({ target, prefix = '', suffix = '', decimals = 2 }: {
  target: number; prefix?: string; suffix?: string; decimals?: number
}) {
  const [display, setDisplay] = useState(target)
  const prev = useRef(target)

  useEffect(() => {
    if (target === prev.current) return
    prev.current = target
    const start = display
    const diff = target - start
    const dur = 600
    const t0 = performance.now()
    let raf: number
    const step = (now: number) => {
      const p = Math.min(1, (now - t0) / dur)
      const ease = 1 - Math.pow(1 - p, 3)
      setDisplay(start + diff * ease)
      if (p < 1) raf = requestAnimationFrame(step)
    }
    raf = requestAnimationFrame(step)
    return () => cancelAnimationFrame(raf)
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [target])

  return <span>{prefix}{display.toFixed(decimals)}{suffix}</span>
}

// ── Protocol bar ──────────────────────────────────────────────────────────────

function ProtocolBar({ protocols }: { protocols: Array<{ key: string; fills: number; realized: number; pct: number }> }) {
  return (
    <div>
      <div className="h-1 w-full overflow-hidden rounded-full bg-[var(--bg-raised)] flex mb-3">
        {protocols.map((p) => (
          <div
            key={p.key}
            style={{
              width: `${p.pct}%`,
              background: protoColor(p.key),
              transition: 'width 600ms ease-out',
            }}
            title={`${protoLabel(p.key)}: ${fmtUsd(p.realized)} (${p.fills} fills)`}
          />
        ))}
      </div>
      <div className="space-y-2">
        {protocols.map((p) => {
          const color = protoColor(p.key)
          return (
            <div key={p.key} className="flex items-center justify-between text-[11px]">
              <div className="flex items-center gap-2">
                <span className="w-2 h-2 rounded-full shrink-0" style={{ background: color }} />
                <span className="font-mono text-[var(--text-secondary)] tracking-[0.06em]">
                  {protoLabel(p.key)}
                </span>
              </div>
              <div className="flex items-center gap-4 font-mono">
                <span className="text-[var(--text-tertiary)]">{p.fills} fills</span>
                <span style={{ color: p.realized > 0 ? 'var(--success)' : 'var(--text-tertiary)' }}>
                  {p.realized > 0 ? '+' : ''}{fmtUsd(p.realized)}
                </span>
              </div>
            </div>
          )
        })}
      </div>
    </div>
  )
}

// ── Component ─────────────────────────────────────────────────────────────────

export default function PortfolioPanel() {
  const [pnl, setPnl] = useState<PnlSummary | null>(null)
  const [rebalancer, setRebalancer] = useState<RebalancerStatus | null>(null)
  const [rebalancerSkipped, setRebalancerSkipped] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)

  const refresh = useCallback(async () => {
    try {
      const r = await fetch(`${SOLVER_API_BASE}/api/solver/pnl`, { cache: 'no-store' })
      if (!r.ok) throw new Error(`HTTP ${r.status}`)
      setPnl(await r.json())
      setError(null)
    } catch (e) {
      setError(e instanceof Error ? e.message : 'fetch failed')
    } finally {
      setLoading(false)
    }
  }, [])

  const refreshRebalancer = useCallback(async () => {
    try {
      const r = await fetch(`${SOLVER_API_BASE}/api/solver/rebalancer/status`, {
        cache: 'no-store',
        headers: authHeaders(),
      })
      if ([401, 403, 503, 404].includes(r.status)) { setRebalancerSkipped(true); return }
      if (!r.ok) return
      setRebalancer(await r.json())
      setRebalancerSkipped(false)
    } catch { /* silent */ }
  }, [])

  useEffect(() => {
    refresh()
    refreshRebalancer()
    const a = setInterval(refresh, POLL_MS)
    const b = setInterval(refreshRebalancer, POLL_MS)
    return () => { clearInterval(a); clearInterval(b) }
  }, [refresh, refreshRebalancer])

  const protocols = useMemo(() => {
    if (!pnl?.by_protocol) return []
    const total = Math.max(pnl.realized_usd_total, 0.001)
    return Object.entries(pnl.by_protocol)
      .map(([key, p]) => ({
        key,
        fills: p.fills,
        realized: p.realized_usd,
        avg: p.avg_profit_usd,
        pct: (p.realized_usd / total) * 100,
      }))
      .sort((a, b) => b.realized - a.realized)
  }, [pnl])

  return (
    <Card padding="none" aria-label="Portfolio">
      <div className="flex items-center justify-between px-4 py-3 border-b border-[var(--border-subtle)]">
        <div className="flex items-center gap-2">
          <Tag>Portfolio</Tag>
          {!error && !loading && (
            <span className="w-1.5 h-1.5 rounded-full bg-[var(--solana-mint)] pulse-live" />
          )}
        </div>
        {error ? (
          <span className="text-[10px] font-mono text-[var(--danger)]">{error}</span>
        ) : pnl ? (
          <span className="font-mono text-[14px] text-[var(--solana-mint)]">
            <RollingNumber target={pnl.realized_usd_total} prefix="+" suffix=" realized" decimals={2} />
          </span>
        ) : null}
      </div>

      <div className="p-4 space-y-5">
        {loading && (
          <div className="text-[var(--text-tertiary)] text-xs text-center py-6 font-mono">
            Loading…
          </div>
        )}

        {!loading && error && (
          <div className="text-[var(--danger)] text-xs text-center py-4 font-mono">{error}</div>
        )}

        {pnl && (
          <>
            {/* KPI strip */}
            <div className="grid grid-cols-3 gap-3">
              <KpiTile
                label="REALIZED P&L"
                value={<RollingNumber target={pnl.realized_usd_total} prefix="$" decimals={2} />}
                tone="mint"
              />
              <KpiTile label="TOTAL FILLS" value={String(pnl.fills_total)} tone="blue" />
              <KpiTile label="LAST 24H" value={String(pnl.last_24h_count)} />
            </div>

            {/* Protocol breakdown */}
            {protocols.length > 0 && (
              <div>
                <div className="text-[10px] tracking-[0.24em] uppercase text-[var(--text-tertiary)] mb-3">
                  Protocol breakdown
                </div>
                <ProtocolBar protocols={protocols} />
              </div>
            )}

            {/* Avg profit per fill */}
            {protocols.length > 0 && (
              <div className="grid grid-cols-3 gap-2">
                {protocols.map((p) => (
                  <div key={p.key} className="bg-[var(--bg-raised)] rounded px-3 py-2">
                    <div className="text-[9px] tracking-[0.2em] uppercase text-[var(--text-tertiary)] mb-1">
                      {protoLabel(p.key)}
                    </div>
                    <div className="font-mono text-[12px]" style={{ color: protoColor(p.key) }}>
                      {fmtUsd(p.avg)} avg
                    </div>
                    <div className="font-mono text-[10px] text-[var(--text-tertiary)]">
                      per fill
                    </div>
                  </div>
                ))}
              </div>
            )}
          </>
        )}

        {/* Rebalancer activity */}
        {!rebalancerSkipped && (
          <RebalancerCard rebalancer={rebalancer} />
        )}
      </div>
    </Card>
  )
}

// ── Sub-components ────────────────────────────────────────────────────────────

function KpiTile({ label, value, tone = 'default' }: {
  label: string
  value: React.ReactNode
  tone?: 'default' | 'mint' | 'blue' | 'danger'
}) {
  const color = {
    default: 'var(--text-primary)',
    mint: 'var(--solana-mint)',
    blue: 'var(--brand-blue)',
    danger: 'var(--danger)',
  }[tone]

  return (
    <div className="bg-[var(--bg-raised)] rounded-[var(--r-md)] px-3 py-2.5">
      <div className="text-[9px] tracking-[0.24em] uppercase text-[var(--text-tertiary)]">{label}</div>
      <div className="font-mono text-[18px] tabular-nums mt-0.5" style={{ color }}>{value}</div>
    </div>
  )
}

function RebalancerCard({ rebalancer }: { rebalancer: RebalancerStatus | null }) {
  if (!rebalancer) return null

  return (
    <div className="border-t border-[var(--border-subtle)] pt-4">
      <div className="text-[10px] tracking-[0.24em] uppercase text-[var(--text-tertiary)] mb-3">
        Rebalancer
      </div>

      <div className="grid grid-cols-3 gap-3 mb-4">
        <div className="bg-[var(--bg-raised)] rounded px-3 py-2">
          <div className="text-[9px] tracking-[0.2em] uppercase text-[var(--text-tertiary)]">Last run</div>
          <div className="font-mono text-[12px] text-[var(--text-primary)] mt-0.5">
            {fmtAge(rebalancer.last_run_at)}
          </div>
          <div className="font-mono text-[9px] text-[var(--text-tertiary)]">cycle #{rebalancer.cycle}</div>
        </div>
        <div className="bg-[var(--bg-raised)] rounded px-3 py-2">
          <div className="text-[9px] tracking-[0.2em] uppercase text-[var(--text-tertiary)]">Next run</div>
          <div className="font-mono text-[12px] text-[var(--text-primary)] mt-0.5">
            {fmtCountdown(rebalancer.next_run_at)}
          </div>
          <div className="font-mono text-[9px] text-[var(--text-tertiary)]">every {rebalancer.interval_secs}s</div>
        </div>
        <div className="bg-[var(--bg-raised)] rounded px-3 py-2">
          <div className="text-[9px] tracking-[0.2em] uppercase text-[var(--text-tertiary)]">Actions</div>
          <div className="font-mono text-[12px] text-[var(--text-primary)] mt-0.5">
            {rebalancer.last_actions.length}
          </div>
        </div>
      </div>

      {rebalancer.last_actions.length > 0 && (
        <div className="space-y-1">
          {rebalancer.last_actions.slice(0, 5).map((entry, i) => {
            const a = entry.action
            const isOk = a.status === 'confirmed' || a.status === 'success'
            const isPending = a.status === 'pending' || !a.status
            const color = isOk ? 'var(--success)' : isPending ? 'var(--brand-blue)' : 'var(--warning)'
            return (
              <div key={i} className="flex items-center gap-2 text-[11px] py-1.5 border-b border-[var(--border-subtle)] last:border-0">
                <span className="w-1.5 h-1.5 rounded-full shrink-0" style={{ background: color }} />
                <span className="font-mono uppercase tracking-[0.06em] text-[var(--brand-blue)] shrink-0 text-[10px]">
                  {a.kind}
                </span>
                <span className="font-mono text-[var(--text-secondary)] shrink-0">
                  {chainName(a.src_chain)} → {chainName(a.dst_chain)}
                </span>
                <span className="font-mono text-[var(--text-tertiary)] text-[10px] shrink-0">
                  {a.token_symbol} {fmtUsd(a.amount_usd)}
                </span>
                <span className="ml-auto font-mono text-[var(--text-tertiary)] text-[10px] shrink-0">
                  {fmtAge(entry.ts)}
                </span>
              </div>
            )
          })}
        </div>
      )}

      {rebalancer.blocked_reason && (
        <div className="mt-2 text-[10px] font-mono text-[var(--warning)] px-2">
          blocked: {rebalancer.blocked_reason}
        </div>
      )}
    </div>
  )
}
