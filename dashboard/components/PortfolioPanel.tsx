'use client'

/**
 * PortfolioPanel — lively multi-chain inventory with spark bars,
 * fill P&L summary, and rebalancer activity feed.
 *
 * Reads from solver-api:
 *   GET /api/solver/portfolio          → chains[], solana_sol_balance, fills
 *   GET /api/solver/rebalancer/status  → cycle, last_actions[]
 *   GET /api/solver/pnl                → realized_usd_total, fills_total
 */

import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { Card, Tag, Badge } from '@/components/ui'
import { chainName } from '@/hooks/useSolverEvents'

// ── API contract ──────────────────────────────────────────────────────────────

interface ChainInventory {
  chain_id: number
  chain_name: string
  native_eth?: number | null
  native_sol?: number | null
  usdc?: number | null
  usdt?: number | null
  weth?: number | null
}

interface PortfolioFillStats {
  confirmed: number
  reverted: number
  active: number
  total_volume_usd: number
  realized_profit_usd: number
}

interface PortfolioResponse {
  solver_address: string
  solana_address?: string | null
  chains: ChainInventory[]
  fills: PortfolioFillStats
  as_of: string
  solana_sol_balance?: number | null
  solana_gas_status?: 'healthy' | 'warn' | 'low_gas' | 'unknown' | null
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

interface PnlSummary {
  realized_usd_total: number
  fills_total: number
  last_24h_count: number
}

// ── Config ────────────────────────────────────────────────────────────────────

const POLL_MS = 15_000
const SOLVER_API_BASE =
  (typeof process !== 'undefined' && process.env?.NEXT_PUBLIC_SOLVER_API_URL) || ''
const SOLVER_API_TOKEN =
  (typeof process !== 'undefined' && process.env?.NEXT_PUBLIC_SOLVER_API_TOKEN) || ''

const SOLANA_CHAIN_ID = 1_399_811_149
const ETH_PRICE_USD = 3500
const SOL_PRICE_USD = 150
const USDC_HEALTHY_USD = 50
const USDC_LOW_USD = 10
const ETH_HEALTHY = 0.005
const ETH_LOW = 0.001

type Status = 'HEALTHY' | 'LOW' | 'CRITICAL'

function chainStatus(row: ChainInventory): Status {
  if (row.chain_id === SOLANA_CHAIN_ID) {
    const sol = row.native_sol ?? 0
    if (sol >= 0.01) return 'HEALTHY'
    if (sol >= 0.005) return 'LOW'
    return 'CRITICAL'
  }
  const usdc = row.usdc ?? 0
  const eth = row.native_eth ?? 0
  let gas: Status = eth < ETH_LOW ? 'CRITICAL' : eth < ETH_HEALTHY ? 'LOW' : 'HEALTHY'
  let stable: Status = usdc < USDC_LOW_USD ? 'CRITICAL' : usdc < USDC_HEALTHY_USD ? 'LOW' : 'HEALTHY'
  const rank = (s: Status) => (s === 'CRITICAL' ? 2 : s === 'LOW' ? 1 : 0)
  return rank(gas) >= rank(stable) ? gas : stable
}

function statusColor(s: Status): string {
  return s === 'HEALTHY' ? 'var(--success)' : s === 'LOW' ? 'var(--warning)' : 'var(--danger)'
}

function rowUsd(r: ChainInventory): number {
  return (r.usdc ?? 0) + (r.usdt ?? 0) +
    (r.native_eth ?? 0) * ETH_PRICE_USD +
    (r.weth ?? 0) * ETH_PRICE_USD +
    (r.native_sol ?? 0) * SOL_PRICE_USD
}

function fmtUsd(n: number): string {
  if (Math.abs(n) >= 1000) return `$${(n / 1000).toFixed(2)}k`
  return `$${n.toFixed(2)}`
}

function fmtNum(n: number | null | undefined, decimals = 2): string {
  if (n == null) return '—'
  if (n === 0) return '0'
  if (Math.abs(n) < 0.0001) return n.toExponential(2)
  return n.toFixed(decimals)
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

// ── Spark bar (mini horizontal bar, animates width changes) ───────────────────

function SparkBar({ pct, color }: { pct: number; color: string }) {
  return (
    <div className="h-0.5 w-20 bg-[var(--bg-raised)] rounded-full overflow-hidden">
      <div
        className="h-full rounded-full"
        style={{
          width: `${Math.min(100, Math.max(0, pct))}%`,
          background: color,
          transition: 'width 600ms var(--ease-out)',
        }}
      />
    </div>
  )
}

// ── Rolling counter (smooth numeric easing) ───────────────────────────────────

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
      const ease = 1 - Math.pow(1 - p, 3) // cubic ease-out
      setDisplay(start + diff * ease)
      if (p < 1) raf = requestAnimationFrame(step)
    }
    raf = requestAnimationFrame(step)
    return () => cancelAnimationFrame(raf)
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [target])

  return (
    <span>
      {prefix}{display.toFixed(decimals)}{suffix}
    </span>
  )
}

// ── Component ─────────────────────────────────────────────────────────────────

export default function PortfolioPanel() {
  const [portfolio, setPortfolio] = useState<PortfolioResponse | null>(null)
  const [rebalancer, setRebalancer] = useState<RebalancerStatus | null>(null)
  const [pnl, setPnl] = useState<PnlSummary | null>(null)
  const [rebalancerSkipped, setRebalancerSkipped] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)

  const refresh = useCallback(async () => {
    try {
      const [pr, pnlr] = await Promise.all([
        fetch(`${SOLVER_API_BASE}/api/solver/portfolio`, { cache: 'no-store' }),
        fetch(`${SOLVER_API_BASE}/api/solver/pnl`, { cache: 'no-store' }),
      ])
      if (!pr.ok) throw new Error(`portfolio HTTP ${pr.status}`)
      const [pd, pnld] = await Promise.all([pr.json(), pnlr.ok ? pnlr.json() : null])
      setPortfolio(pd)
      if (pnld) setPnl(pnld)
      setError(null)
    } catch (e) {
      setError(e instanceof Error ? e.message : 'unknown error')
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

  // Derived: sort chains by USD desc, compute max for spark bars
  const sortedChains = useMemo(() => {
    if (!portfolio) return []
    return [...portfolio.chains].sort((a, b) => rowUsd(b) - rowUsd(a))
  }, [portfolio])

  const maxUsd = useMemo(
    () => Math.max(...sortedChains.map(rowUsd), 1),
    [sortedChains],
  )

  const totalUsd = useMemo(
    () => sortedChains.reduce((a, r) => a + rowUsd(r), 0),
    [sortedChains],
  )

  const criticalCount = sortedChains.filter((r) => chainStatus(r) === 'CRITICAL').length
  const healthyCount = sortedChains.filter((r) => chainStatus(r) === 'HEALTHY').length

  return (
    <Card padding="none" aria-label="Portfolio">
      {/* Header */}
      <div className="flex items-center justify-between px-4 py-3 border-b border-[var(--border-subtle)]">
        <div className="flex items-center gap-2">
          <Tag>Portfolio</Tag>
          {!error && (
            <span className="w-1.5 h-1.5 rounded-full bg-[var(--solana-mint)] pulse-live" />
          )}
          {criticalCount > 0 && (
            <Badge tone="danger" dot pulse>{criticalCount} CRITICAL</Badge>
          )}
        </div>
        <div className="flex items-baseline gap-3">
          {portfolio && (
            <span className="font-mono text-[10px] text-[var(--text-tertiary)]">
              {sortedChains.length} chains · {fmtAge(portfolio.as_of)}
            </span>
          )}
          <span className="font-mono text-[14px] text-[var(--solana-mint)]">
            {loading && !portfolio ? '—' : fmtUsd(totalUsd)}
          </span>
        </div>
      </div>

      <div className="p-4 space-y-5">
        {/* KPI strip */}
        <div className="grid grid-cols-2 sm:grid-cols-4 gap-3">
          <KpiTile
            label="TOTAL USD"
            value={<RollingNumber target={totalUsd} prefix="$" decimals={2} />}
            tone="mint"
          />
          <KpiTile
            label="REALIZED P&L"
            value={pnl ? fmtUsd(pnl.realized_usd_total) : '—'}
            tone={pnl && pnl.realized_usd_total >= 0 ? 'mint' : 'danger'}
          />
          <KpiTile
            label="FILLS"
            value={pnl ? String(pnl.fills_total) : (portfolio?.fills.confirmed ?? 0).toString()}
            tone="blue"
          />
          <KpiTile
            label="CHAINS"
            value={`${healthyCount}/${sortedChains.length}`}
            tone={criticalCount > 0 ? 'danger' : 'default'}
          />
        </div>

        {/* Per-chain inventory with spark bars */}
        {loading && !portfolio && (
          <div className="text-[var(--text-tertiary)] text-xs text-center py-6 font-mono">
            Fetching chain inventory…
          </div>
        )}
        {error && !portfolio && (
          <div className="text-[var(--danger)] text-xs text-center py-4 font-mono">
            {error}
          </div>
        )}
        {portfolio && sortedChains.length === 0 && (
          <div className="text-[var(--text-tertiary)] text-xs text-center py-6 font-mono">
            No chain data yet — set SOLVER_ADDRESS on solver-api and refresh.
          </div>
        )}

        {sortedChains.length > 0 && (
          <div>
            <div className="text-[10px] tracking-[0.24em] uppercase text-[var(--text-tertiary)] mb-2">
              Chain inventory
            </div>
            <div className="space-y-1">
              {sortedChains.map((row) => {
                const status = chainStatus(row)
                const color = statusColor(status)
                const usd = rowUsd(row)
                const pct = (usd / maxUsd) * 100
                const isSolana = row.chain_id === SOLANA_CHAIN_ID
                const displayName = row.chain_name || chainName(row.chain_id)
                const stables = (row.usdc ?? 0) + (row.usdt ?? 0)
                const gas = isSolana
                  ? `${fmtNum(row.native_sol, 4)} SOL`
                  : `${fmtNum(row.native_eth, 4)} ETH`

                return (
                  <div
                    key={row.chain_id}
                    className="flex items-center gap-3 py-2 text-[11px] border-b border-[var(--border-subtle)] last:border-0"
                  >
                    {/* Status dot */}
                    <span
                      className="w-1.5 h-1.5 rounded-full shrink-0"
                      style={{ background: color }}
                      title={status}
                    />

                    {/* Chain name */}
                    <span className="font-mono w-16 shrink-0 text-[var(--text-primary)] uppercase tracking-[0.08em]">
                      {displayName}
                    </span>

                    {/* Spark bar */}
                    <SparkBar pct={pct} color={color} />

                    {/* Balances */}
                    <span className="font-mono text-[var(--text-secondary)] shrink-0">
                      {stables > 0 ? `$${stables.toFixed(0)} stables` : '—'}
                    </span>
                    <span className="font-mono text-[var(--text-tertiary)] text-[10px] shrink-0">
                      {gas}
                    </span>

                    {/* USD total */}
                    <span className="font-mono ml-auto shrink-0" style={{ color: usd > 0 ? 'var(--text-primary)' : 'var(--text-disabled)' }}>
                      {fmtUsd(usd)}
                    </span>

                    {/* Status badge */}
                    <span
                      className="text-[9px] font-mono px-1.5 py-0.5 rounded shrink-0 hidden sm:block"
                      style={{ color, background: `${color}18` }}
                    >
                      {status}
                    </span>
                  </div>
                )
              })}
            </div>
          </div>
        )}

        {/* Solana gas alert */}
        {portfolio?.solana_gas_status && portfolio.solana_gas_status !== 'unknown' && (
          <SolanaGasAlert
            status={portfolio.solana_gas_status}
            solBalance={portfolio.solana_sol_balance}
            address={portfolio.solana_address}
          />
        )}

        {/* Rebalancer activity feed */}
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

function SolanaGasAlert({ status, solBalance, address }: {
  status: NonNullable<PortfolioResponse['solana_gas_status']>
  solBalance?: number | null
  address?: string | null
}) {
  const isCrit = status === 'low_gas'
  const isWarn = status === 'warn'
  const color = isCrit ? 'var(--danger)' : isWarn ? 'var(--warning)' : 'var(--success)'
  const bg = isCrit ? 'rgba(255,68,68,0.06)' : isWarn ? 'rgba(255,184,0,0.06)' : 'rgba(0,255,136,0.04)'
  const label = isCrit ? 'LOW GAS' : isWarn ? 'WARN' : 'HEALTHY'
  return (
    <div
      className="flex items-center gap-2 text-[11px] font-mono px-3 py-2 rounded border"
      style={{ borderColor: color, color, background: bg }}
    >
      <span>⛽</span>
      <span>Solana gas <strong>{label}</strong></span>
      {solBalance != null && <span className="text-[10px] opacity-70">· {solBalance.toFixed(6)} SOL</span>}
      {address && (
        <span className="ml-auto text-[10px] opacity-60 font-mono hidden sm:block">
          {address.slice(0, 8)}…{address.slice(-4)}
        </span>
      )}
    </div>
  )
}

function RebalancerCard({ rebalancer }: { rebalancer: RebalancerStatus | null }) {
  return (
    <div className="border-t border-[var(--border-subtle)] pt-4">
      <div className="text-[10px] tracking-[0.24em] uppercase text-[var(--text-tertiary)] mb-3">
        Rebalancer
      </div>

      {!rebalancer && (
        <div className="text-[var(--text-tertiary)] text-xs font-mono">
          Loading rebalancer status…
        </div>
      )}

      {rebalancer && (
        <>
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
              {rebalancer.blocked_reason && (
                <div className="font-mono text-[9px] text-[var(--warning)]">
                  blocked
                </div>
              )}
            </div>
          </div>

          {rebalancer.last_actions.length > 0 && (
            <div className="space-y-1">
              {rebalancer.last_actions.slice(0, 5).map((entry, i) => {
                const a = entry.action
                const srcName = chainName(a.src_chain)
                const dstName = chainName(a.dst_chain)
                const isPending = a.status === 'pending' || !a.status
                const isOk = a.status === 'confirmed' || a.status === 'success'
                const statusColor = isOk
                  ? 'var(--success)'
                  : isPending
                    ? 'var(--brand-blue)'
                    : 'var(--warning)'
                return (
                  <div
                    key={i}
                    className="flex items-center gap-2 text-[11px] py-1.5 border-b border-[var(--border-subtle)] last:border-0"
                  >
                    <span
                      className="w-1.5 h-1.5 rounded-full shrink-0"
                      style={{ background: statusColor }}
                    />
                    <span className="font-mono uppercase tracking-[0.06em] text-[var(--brand-blue)] shrink-0 text-[10px]">
                      {a.kind}
                    </span>
                    <span className="font-mono text-[var(--text-secondary)] shrink-0">
                      {srcName} → {dstName}
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

          {rebalancer.last_actions.length === 0 && (
            <div className="text-[var(--text-tertiary)] text-[11px] font-mono text-center py-3 bg-[var(--bg-raised)] rounded">
              No rebalancer actions yet
            </div>
          )}

          {rebalancer.blocked_reason && (
            <div className="mt-2 text-[10px] font-mono text-[var(--warning)] px-2">
              blocked: {rebalancer.blocked_reason}
            </div>
          )}
        </>
      )}
    </div>
  )
}
