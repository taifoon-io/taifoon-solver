'use client'

/**
 * Portfolio tab — per-chain inventory + Solana row + rebalancer activity.
 *
 * Reads from solver-api:
 *   GET /api/solver/portfolio          → chains[], solana_sol_balance, solana_gas_status
 *   GET /api/solver/rebalancer/status  → last_run_at, next_run_at, last_actions[]
 *
 * /api/solver/portfolio is public; /api/solver/rebalancer/status is on the
 * token-gated `protected` sub-router. The rebalancer card auto-skips render
 * on a 401/403/503 — the brief explicitly says "if it exists, otherwise skip."
 *
 * Polls portfolio every 30s, rebalancer every 30s. Both retry quietly on
 * transient failure (no UI flash).
 */

import { useCallback, useEffect, useState } from 'react'
import { Card, CardHeader, Tag, Badge } from '@/components/ui'

// ── API contract ─────────────────────────────────────────────────────────────

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

// ── Config ───────────────────────────────────────────────────────────────────

const POLL_MS = 30_000
const SOLVER_API_BASE =
  (typeof process !== 'undefined' && process.env?.NEXT_PUBLIC_SOLVER_API_URL) || ''
const SOLVER_API_TOKEN =
  (typeof process !== 'undefined' && process.env?.NEXT_PUBLIC_SOLVER_API_TOKEN) || ''

const SOLANA_CHAIN_ID = 1_399_811_149

// USDC stable-balance thresholds. Mirrors portfolio_sidecar::inventory targets:
// any chain with >= $50 USDC is healthy, >= $10 is low (operational floor),
// below $10 is critical (will skip fills). These are operator-facing flags,
// not the precise sidecar targets — the sidecar uses per-chain target USD
// from chain_wiring.json, which the dashboard doesn't load.
const USDC_HEALTHY_USD = 50
const USDC_LOW_USD = 10
// Native gas thresholds in ETH-equivalent. 0.005 ETH ≈ ~$15 covers ~50 fills
// at typical Arb/Base gas; below 0.001 is critical.
const ETH_HEALTHY = 0.005
const ETH_LOW = 0.001

type Status = 'HEALTHY' | 'LOW' | 'CRITICAL'

function chainStatus(row: ChainInventory): Status {
  // For the Solana row, defer to native_sol (USDC-SPL is informational).
  if (row.chain_id === SOLANA_CHAIN_ID) {
    const sol = row.native_sol ?? 0
    if (sol >= 0.01) return 'HEALTHY'
    if (sol >= 0.005) return 'LOW'
    return 'CRITICAL'
  }
  const usdc = row.usdc ?? 0
  const eth = row.native_eth ?? 0
  // Both gas and stables matter; degrade to the worse of the two.
  let gas: Status = 'HEALTHY'
  if (eth < ETH_LOW) gas = 'CRITICAL'
  else if (eth < ETH_HEALTHY) gas = 'LOW'
  let stable: Status = 'HEALTHY'
  if (usdc < USDC_LOW_USD) stable = 'CRITICAL'
  else if (usdc < USDC_HEALTHY_USD) stable = 'LOW'
  const rank = (s: Status) => (s === 'CRITICAL' ? 2 : s === 'LOW' ? 1 : 0)
  return rank(gas) >= rank(stable) ? gas : stable
}

function statusColor(s: Status): string {
  return s === 'HEALTHY'
    ? 'var(--success)'
    : s === 'LOW'
      ? 'var(--warning)'
      : 'var(--danger)'
}

function gasStatusToStatus(g: PortfolioResponse['solana_gas_status']): Status | null {
  if (!g || g === 'unknown') return null
  if (g === 'healthy') return 'HEALTHY'
  if (g === 'warn') return 'LOW'
  return 'CRITICAL'
}

// Lightweight USD estimate so the summary card has a number; the API doesn't
// return on-chain prices. Stables count 1:1; ETH at $3.5k, SOL at $150 ballpark.
// These are display-only — operator-grade pricing belongs in solver-api when
// it's wired up.
const ETH_PRICE_USD = 3500
const SOL_PRICE_USD = 150
function rowUsd(r: ChainInventory): number {
  const stable = (r.usdc ?? 0) + (r.usdt ?? 0)
  const eth = (r.native_eth ?? 0) * ETH_PRICE_USD
  const weth = (r.weth ?? 0) * ETH_PRICE_USD
  const sol = (r.native_sol ?? 0) * SOL_PRICE_USD
  return stable + eth + weth + sol
}

function fmtUsd(n: number): string {
  if (n >= 1000) return `$${(n / 1000).toFixed(2)}k`
  return `$${n.toFixed(2)}`
}

function fmtNum(n: number | null | undefined, decimals = 2): string {
  if (n === null || n === undefined) return '—'
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
  const m = Math.floor(s / 60)
  return `in ${m}m`
}

function authHeaders(): HeadersInit {
  return SOLVER_API_TOKEN
    ? { Authorization: `Bearer ${SOLVER_API_TOKEN}` }
    : {}
}

// ── Component ────────────────────────────────────────────────────────────────

export default function PortfolioPanel() {
  const [portfolio, setPortfolio] = useState<PortfolioResponse | null>(null)
  const [rebalancer, setRebalancer] = useState<RebalancerStatus | null>(null)
  const [rebalancerSkipped, setRebalancerSkipped] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)

  const refreshPortfolio = useCallback(async () => {
    try {
      const r = await fetch(`${SOLVER_API_BASE}/api/solver/portfolio`, {
        cache: 'no-store',
      })
      if (!r.ok) throw new Error(`portfolio HTTP ${r.status}`)
      const d: PortfolioResponse = await r.json()
      setPortfolio(d)
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
      // Brief: "if it exists, otherwise skip" — treat 401/403/503/404 as "skip the card."
      if (r.status === 401 || r.status === 403 || r.status === 503 || r.status === 404) {
        setRebalancerSkipped(true)
        return
      }
      if (!r.ok) return // transient — keep prior data
      const d: RebalancerStatus = await r.json()
      setRebalancer(d)
      setRebalancerSkipped(false)
    } catch {
      // network error — keep silent and try again next tick
    }
  }, [])

  useEffect(() => {
    refreshPortfolio()
    refreshRebalancer()
    const a = setInterval(refreshPortfolio, POLL_MS)
    const b = setInterval(refreshRebalancer, POLL_MS)
    return () => {
      clearInterval(a)
      clearInterval(b)
    }
  }, [refreshPortfolio, refreshRebalancer])

  const totalUsd = portfolio?.chains.reduce((acc, r) => acc + rowUsd(r), 0) ?? 0
  const chainCount = portfolio?.chains.length ?? 0
  const criticalCount =
    portfolio?.chains.filter((r) => chainStatus(r) === 'CRITICAL').length ?? 0

  return (
    <Card padding="none">
      <div className="flex items-center justify-between px-4 py-3 border-b border-[var(--border-subtle)]">
        <div className="flex items-center gap-2">
          <Tag>Portfolio</Tag>
          {portfolio && (
            <span className="text-[10px] font-mono text-[var(--text-tertiary)]">
              {chainCount} chains · refreshed {fmtAge(portfolio.as_of)}
            </span>
          )}
          {criticalCount > 0 && (
            <Badge tone="danger" dot pulse>
              {criticalCount} CRITICAL
            </Badge>
          )}
        </div>
        <div className="flex items-baseline gap-2">
          <span className="font-mono text-[10px] tracking-[0.2em] uppercase text-[var(--text-tertiary)]">
            TOTAL
          </span>
          <span className="font-mono text-[14px] text-[var(--solana-mint)]">
            {fmtUsd(totalUsd)}
          </span>
        </div>
      </div>

      <div className="p-4 space-y-4">
        {/* Summary card */}
        <div className="grid grid-cols-2 sm:grid-cols-4 gap-3">
          <SummaryTile label="TOTAL USD" value={fmtUsd(totalUsd)} />
          <SummaryTile label="CHAINS" value={chainCount.toString()} />
          <SummaryTile
            label="HEALTHY"
            value={
              portfolio
                ? portfolio.chains.filter((r) => chainStatus(r) === 'HEALTHY').length.toString()
                : '—'
            }
            tone="success"
          />
          <SummaryTile
            label="CRITICAL"
            value={criticalCount.toString()}
            tone={criticalCount > 0 ? 'danger' : 'muted'}
          />
        </div>

        {/* Per-chain table */}
        {loading && !portfolio && (
          <div className="text-[var(--text-tertiary)] text-xs text-center py-6">
            Loading portfolio…
          </div>
        )}
        {error && !portfolio && (
          <div className="text-[var(--danger)] text-xs text-center py-4 font-mono">
            Portfolio fetch failed: {error}
          </div>
        )}
        {portfolio && portfolio.chains.length === 0 && (
          <div className="text-[var(--text-tertiary)] text-xs text-center py-6">
            No chain data yet. Set SOLVER_ADDRESS on solver-api and refresh.
          </div>
        )}
        {portfolio && portfolio.chains.length > 0 && (
          <div className="overflow-x-auto">
            <table className="w-full text-[12px] font-mono">
              <thead>
                <tr className="text-[10px] tracking-[0.16em] uppercase text-[var(--text-tertiary)] border-b border-[var(--border-subtle)]">
                  <th className="text-left py-2 pr-3">Chain</th>
                  <th className="text-right py-2 pr-3">USDC</th>
                  <th className="text-right py-2 pr-3">USDT</th>
                  <th className="text-right py-2 pr-3">WETH</th>
                  <th className="text-right py-2 pr-3">Gas</th>
                  <th className="text-right py-2 pr-3">USD</th>
                  <th className="text-right py-2">Status</th>
                </tr>
              </thead>
              <tbody>
                {portfolio.chains.map((row) => (
                  <ChainRow key={row.chain_id} row={row} />
                ))}
              </tbody>
            </table>
          </div>
        )}

        {/* Solana gas status surfaced separately from the row when set. */}
        {portfolio?.solana_gas_status && portfolio.solana_gas_status !== 'unknown' && (
          <div
            className="text-[11px] font-mono px-3 py-2 rounded border"
            style={{
              borderColor:
                gasStatusToStatus(portfolio.solana_gas_status) === 'CRITICAL'
                  ? 'var(--danger)'
                  : gasStatusToStatus(portfolio.solana_gas_status) === 'LOW'
                    ? 'var(--warning)'
                    : 'var(--success)',
              color:
                gasStatusToStatus(portfolio.solana_gas_status) === 'CRITICAL'
                  ? 'var(--danger)'
                  : gasStatusToStatus(portfolio.solana_gas_status) === 'LOW'
                    ? 'var(--warning)'
                    : 'var(--success)',
              background:
                gasStatusToStatus(portfolio.solana_gas_status) === 'CRITICAL'
                  ? 'rgba(255, 68, 68, 0.06)'
                  : gasStatusToStatus(portfolio.solana_gas_status) === 'LOW'
                    ? 'rgba(255, 184, 0, 0.06)'
                    : 'rgba(0, 255, 136, 0.04)',
            }}
          >
            ⛽ Solana gas {portfolio.solana_gas_status.toUpperCase()} ·
            {' '}
            {fmtNum(portfolio.solana_sol_balance, 6)} SOL on{' '}
            {portfolio.solana_address ?? '(unset)'}
          </div>
        )}

        {/* Rebalancer card */}
        {!rebalancerSkipped && (
          <div className="border-t border-[var(--border-subtle)] pt-3">
            <CardHeader title="Rebalancer" />
            {!rebalancer && (
              <div className="text-[var(--text-tertiary)] text-xs">Loading rebalancer status…</div>
            )}
            {rebalancer && (
              <div className="grid grid-cols-1 sm:grid-cols-3 gap-3">
                <div className="text-[11px]">
                  <div className="text-[10px] tracking-[0.16em] uppercase text-[var(--text-tertiary)]">
                    LAST RUN
                  </div>
                  <div className="font-mono text-[var(--text-primary)] mt-0.5">
                    {fmtAge(rebalancer.last_run_at)}
                  </div>
                  <div className="font-mono text-[10px] text-[var(--text-tertiary)] mt-0.5">
                    cycle #{rebalancer.cycle}
                  </div>
                </div>
                <div className="text-[11px]">
                  <div className="text-[10px] tracking-[0.16em] uppercase text-[var(--text-tertiary)]">
                    NEXT RUN
                  </div>
                  <div className="font-mono text-[var(--text-primary)] mt-0.5">
                    {fmtCountdown(rebalancer.next_run_at)}
                  </div>
                  <div className="font-mono text-[10px] text-[var(--text-tertiary)] mt-0.5">
                    every {rebalancer.interval_secs}s
                  </div>
                </div>
                <div className="text-[11px]">
                  <div className="text-[10px] tracking-[0.16em] uppercase text-[var(--text-tertiary)]">
                    LAST ACTION
                  </div>
                  {rebalancer.last_actions[0] ? (
                    <BridgeActionLine entry={rebalancer.last_actions[0]} />
                  ) : (
                    <div className="font-mono text-[var(--text-tertiary)] mt-0.5">
                      no actions yet
                    </div>
                  )}
                  {rebalancer.blocked_reason && (
                    <div className="font-mono text-[10px] text-[var(--warning)] mt-0.5">
                      blocked: {rebalancer.blocked_reason}
                    </div>
                  )}
                </div>
              </div>
            )}
          </div>
        )}
      </div>
    </Card>
  )
}

// ── Subcomponents ────────────────────────────────────────────────────────────

function SummaryTile({
  label,
  value,
  tone = 'default',
}: {
  label: string
  value: string
  tone?: 'default' | 'success' | 'danger' | 'muted'
}) {
  const color =
    tone === 'success'
      ? 'var(--success)'
      : tone === 'danger'
        ? 'var(--danger)'
        : tone === 'muted'
          ? 'var(--text-tertiary)'
          : 'var(--text-primary)'
  return (
    <div className="bg-[var(--bg-raised)] rounded px-3 py-2">
      <div className="text-[9px] tracking-[0.2em] uppercase text-[var(--text-tertiary)]">
        {label}
      </div>
      <div className="font-mono text-[16px] mt-1" style={{ color }}>
        {value}
      </div>
    </div>
  )
}

function ChainRow({ row }: { row: ChainInventory }) {
  const status = chainStatus(row)
  const color = statusColor(status)
  const usd = rowUsd(row)
  const isSolana = row.chain_id === SOLANA_CHAIN_ID
  const gasDisplay = isSolana
    ? `${fmtNum(row.native_sol, 4)} SOL`
    : `${fmtNum(row.native_eth, 4)} ETH`
  return (
    <tr className="border-b border-[var(--border-subtle)] hover:bg-[var(--bg-raised)]">
      <td className="py-2 pr-3">
        <div className="flex items-center gap-2">
          <span
            className="w-1.5 h-1.5 rounded-full"
            style={{ background: color }}
            aria-hidden
          />
          <span className="text-[var(--text-primary)]">{row.chain_name}</span>
          <span className="text-[9px] text-[var(--text-tertiary)]">{row.chain_id}</span>
        </div>
      </td>
      <td className="text-right py-2 pr-3 text-[var(--text-secondary)]">
        {fmtNum(row.usdc, 2)}
      </td>
      <td className="text-right py-2 pr-3 text-[var(--text-secondary)]">
        {fmtNum(row.usdt, 2)}
      </td>
      <td className="text-right py-2 pr-3 text-[var(--text-secondary)]">
        {fmtNum(row.weth, 4)}
      </td>
      <td className="text-right py-2 pr-3 text-[var(--text-secondary)]">{gasDisplay}</td>
      <td className="text-right py-2 pr-3 text-[var(--text-primary)]">{fmtUsd(usd)}</td>
      <td className="text-right py-2">
        <span
          className="text-[10px] font-bold px-1.5 py-0.5 rounded"
          style={{ color, background: `${color}1a` }}
        >
          {status}
        </span>
      </td>
    </tr>
  )
}

function BridgeActionLine({ entry }: { entry: ActionLogEntry }) {
  const a = entry.action
  return (
    <div className="font-mono text-[var(--text-primary)] mt-0.5">
      <div>
        {a.kind} · {a.token_symbol} · {fmtUsd(a.amount_usd)}
      </div>
      <div className="text-[10px] text-[var(--text-tertiary)]">
        {a.src_chain} → {a.dst_chain} · {fmtAge(entry.ts)}
        {a.status ? ` · ${a.status}` : ''}
      </div>
    </div>
  )
}
