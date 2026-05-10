'use client'

import Link from 'next/link'
import { useEffect, useMemo, useRef, useState } from 'react'
import { NavBar, Footer, Card, Button, Badge, StatTile, Tag } from '@/components/ui'
import { protocolColors } from '@/lib/tokens'

type SolverStatus = 'live' | 'offline' | 'connecting'

interface PortfolioResponse {
  solver_address: string
  solana_address?: string | null
  chains: Array<{ chain_id: number; chain_name: string }>
  fills?: { confirmed: number; reverted: number }
}

interface PnlSummary {
  realized_usd_total: number
  fills_total: number
  last_24h_count: number
  by_protocol: Record<string, { fills: number; realized_usd: number; avg_profit_usd: number }>
}

interface OutcomeRecord {
  ts: string
  protocol: string
  decision: string
  src_chain: number
  dst_chain: number
  tx_hash: string | null
  actual_profit_usd: number | null
}

interface HostedSolver {
  solver_id: string
  name: string
  evm_address: string
  signing_mode: string
  chains: string
  protocols: string
  registered_at: string
  active: boolean
  donut_accrued_usd: number
}

const SOLVER_API_BASE =
  (typeof process !== 'undefined' && process.env?.NEXT_PUBLIC_SOLVER_API_URL) || ''

const POLL_INTERVAL_MS = 5000
const SSE_OFFLINE_THRESHOLD_MS = 30_000
const SSE_HEALTH_CHECK_MS = 5000

const CHAIN_LABEL: Record<number, string> = {
  1: 'ETH', 10: 'OP', 137: 'MATIC', 8453: 'BASE', 42161: 'ARB',
  56: 'BSC', 59144: 'LINEA', 324: 'ZKSYNC', 1399811149: 'SOL',
}

function chainLabel(id: number): string {
  return CHAIN_LABEL[id] ?? `c${id}`
}

function shortAddr(a: string): string {
  if (a.length <= 12) return a
  return `${a.slice(0, 6)}…${a.slice(-4)}`
}

function fmtAge(iso: string | null): string {
  if (!iso) return '—'
  const ms = Date.now() - new Date(iso).getTime()
  if (Number.isNaN(ms) || ms < 0) return 'now'
  const s = Math.floor(ms / 1000)
  if (s < 60) return `${s}s ago`
  const m = Math.floor(s / 60)
  if (m < 60) return `${m}m ago`
  const h = Math.floor(m / 60)
  if (h < 24) return `${h}h ago`
  return `${Math.floor(h / 24)}d ago`
}

type Filter = 'all' | 'live' | 'offline'

export default function PortalPage() {
  const [filter, setFilter] = useState<Filter>('all')
  const [portfolio, setPortfolio] = useState<PortfolioResponse | null>(null)
  const [pnl, setPnl] = useState<PnlSummary | null>(null)
  const [lastFill, setLastFill] = useState<OutcomeRecord | null>(null)
  const [status, setStatus] = useState<SolverStatus>('connecting')
  const [loadError, setLoadError] = useState<string | null>(null)
  const [hostedSolvers, setHostedSolvers] = useState<HostedSolver[]>([])

  const lastSseAtRef = useRef<number>(Date.now())

  useEffect(() => {
    let cancelled = false

    const refresh = async () => {
      try {
        const [pRes, plRes, oRes, hRes] = await Promise.all([
          fetch(`${SOLVER_API_BASE}/api/solver/portfolio`, { cache: 'no-store' }),
          fetch(`${SOLVER_API_BASE}/api/solver/pnl`, { cache: 'no-store' }),
          fetch(`${SOLVER_API_BASE}/api/solver/outcomes?limit=1`, { cache: 'no-store' }),
          fetch(`${SOLVER_API_BASE}/api/hosting/solvers`, { cache: 'no-store' }),
        ])
        if (cancelled) return
        if (pRes.ok) setPortfolio(await pRes.json())
        if (plRes.ok) setPnl(await plRes.json())
        if (oRes.ok) {
          const recs = (await oRes.json()) as OutcomeRecord[]
          setLastFill(recs[0] ?? null)
        }
        if (hRes.ok) {
          const d = await hRes.json()
          setHostedSolvers(d.solvers ?? [])
        }
        setLoadError(null)
      } catch (e) {
        if (!cancelled) setLoadError(e instanceof Error ? e.message : String(e))
      }
    }

    refresh()
    const poll = setInterval(refresh, POLL_INTERVAL_MS)

    const es = new EventSource(`${SOLVER_API_BASE}/api/solver/stream`)
    const markSeen = () => {
      lastSseAtRef.current = Date.now()
      setStatus('live')
    }
    es.onopen = markSeen
    es.onmessage = markSeen
    es.onerror = () => {
      const age = Date.now() - lastSseAtRef.current
      setStatus(age > SSE_OFFLINE_THRESHOLD_MS ? 'offline' : 'connecting')
    }

    const health = setInterval(() => {
      const age = Date.now() - lastSseAtRef.current
      if (age > SSE_OFFLINE_THRESHOLD_MS) setStatus('offline')
    }, SSE_HEALTH_CHECK_MS)

    return () => {
      cancelled = true
      clearInterval(poll)
      clearInterval(health)
      es.close()
    }
  }, [])

  const solver = useMemo(() => {
    if (!portfolio) return null
    const protocols = pnl ? Object.keys(pnl.by_protocol) : []
    const chainIds = portfolio.chains.map((c) => c.chain_id)
    const totalPnl = pnl?.realized_usd_total ?? 0
    return {
      address: portfolio.solver_address,
      solanaAddress: portfolio.solana_address ?? null,
      status,
      protocols,
      chains: chainIds.map(chainLabel),
      pnl_total: totalPnl,
      fills_total: pnl?.fills_total ?? 0,
      fills_24h: pnl?.last_24h_count ?? 0,
      last_fill_ts: lastFill?.ts ?? null,
      last_fill_protocol: lastFill?.protocol ?? null,
      last_fill_tx: lastFill?.tx_hash ?? null,
      last_fill_profit: lastFill?.actual_profit_usd ?? null,
    }
  }, [portfolio, pnl, lastFill, status])

  const visible = useMemo(() => {
    if (!solver) return []
    if (filter === 'all') return [solver]
    if (filter === 'live' && solver.status === 'live') return [solver]
    if (filter === 'offline' && solver.status === 'offline') return [solver]
    return []
  }, [solver, filter])

  const fleetCount = Math.max(solver ? 1 : 0, hostedSolvers.length)
  const liveCount = solver?.status === 'live' ? 1 : 0
  const fillsTotal = solver?.fills_total ?? 0
  const totalPnl = solver?.pnl_total ?? 0

  return (
    <>
      <NavBar />
      <main className="flex-1">
        {/* Header */}
        <div className="border-b border-[var(--border-subtle)]">
          <div className="max-w-[1400px] mx-auto px-6 py-12 flex items-end justify-between flex-wrap gap-6">
            <div>
              <Tag>The portal</Tag>
              <h1 className="tf-display tf-gradient-silver mt-4 text-[clamp(2rem,4vw,3rem)]">
                Solver fleet.
              </h1>
              <p className="mt-3 text-sm text-[var(--text-secondary)] max-w-[560px] leading-relaxed">
                Live state of the solver this dashboard is wired to. Address,
                fill count, and P&amp;L come straight from the running solver.
                LIVE/OFFLINE reflects the SSE event stream.
              </p>
            </div>
            <Button href="/onboard" variant="primary" size="lg">
              <span className="text-[var(--text-tertiary)]">+</span>
              SPIN UP NEW SOLVER →
            </Button>
          </div>

          <div className="max-w-[1400px] mx-auto px-6 pb-10 grid grid-cols-2 sm:grid-cols-4 gap-x-12 gap-y-6">
            <StatTile label="FLEET" value={fleetCount} />
            <StatTile label="LIVE" value={liveCount} tone="mint" />
            <StatTile label="FILLS" value={fillsTotal} tone="blue" />
            <StatTile
              label="P&L REALIZED"
              value={`$${totalPnl.toFixed(2)}`}
              tone={totalPnl >= 0 ? 'mint' : 'danger'}
            />
          </div>
        </div>

        {/* Filters */}
        <div className="max-w-[1400px] mx-auto px-6 pt-8 flex items-center gap-4">
          {(['all', 'live', 'offline'] as Filter[]).map((f) => (
            <button
              key={f}
              onClick={() => setFilter(f)}
              className={`font-mono text-[11px] tracking-[0.2em] uppercase transition-colors ${
                filter === f
                  ? 'text-[var(--brand-blue)]'
                  : 'text-[var(--text-tertiary)] hover:text-[var(--text-primary)]'
              }`}
            >
              {filter === f ? `[ ${f} ]` : f}
            </button>
          ))}
        </div>

        {/* Solver list */}
        <div className="max-w-[1400px] mx-auto px-6 py-8 space-y-3">
          {!solver && !loadError && (
            <Card padding="lg">
              <div className="flex flex-col items-center py-8 gap-4">
                <div className="w-12 h-12 rounded-full border border-[var(--border-default)] flex items-center justify-center">
                  <span className="w-2 h-2 rounded-full bg-[var(--brand-blue)] animate-pulse" />
                </div>
                <div className="text-center">
                  <div className="text-sm text-[var(--text-secondary)] font-mono">Connecting to solver…</div>
                  <div className="text-[11px] text-[var(--text-tertiary)] mt-1">
                    Make sure the solver API is running on port 8082
                  </div>
                </div>
              </div>
            </Card>
          )}
          {loadError && (
            <Card padding="lg">
              <div className="flex flex-col items-center py-8 gap-4">
                <div className="w-12 h-12 rounded-full border border-[var(--danger)]/40 flex items-center justify-center bg-[rgba(255,107,107,0.05)]">
                  <span className="text-[var(--danger)] font-mono text-lg">!</span>
                </div>
                <div className="text-center max-w-[400px]">
                  <div className="text-sm text-[var(--danger)] font-mono">Solver API unreachable</div>
                  <div className="text-[11px] text-[var(--text-tertiary)] mt-1 font-mono">{loadError}</div>
                  <div className="text-[11px] text-[var(--text-tertiary)] mt-2">
                    Start the solver with{' '}
                    <code className="bg-[var(--bg-raised)] px-1 py-0.5 rounded font-mono">cargo run --bin taifoon-solver</code>
                  </div>
                </div>
              </div>
            </Card>
          )}
          {solver && visible.length === 0 && (
            <Card padding="lg" className="text-center">
              <div className="text-[var(--text-secondary)] text-sm">
                No solvers match the &ldquo;{filter}&rdquo; filter.
              </div>
            </Card>
          )}
          {visible.map((s) => {
            const hostedMatch = hostedSolvers.find(
              (h) => h.evm_address.toLowerCase() === s.address.toLowerCase()
            )
            return (
              <SolverCard key={s.address} solver={s} hostedId={hostedMatch?.solver_id} />
            )
          })}
          {hostedSolvers.filter(h => !solver || h.evm_address.toLowerCase() !== solver.address.toLowerCase()).map((h) => (
            <HostedSolverCard key={h.solver_id} hosted={h} />
          ))}
        </div>

        {/* Onboarding hint */}
        <div className="max-w-[1400px] mx-auto px-6 pb-16">
          <Card padding="lg" accent>
            <div className="flex items-center justify-between flex-wrap gap-4">
              <div>
                <Tag>Onboard another team</Tag>
                <h3 className="mt-3 text-xl font-light text-[var(--text-primary)]">
                  Add a solver in three phases.
                </h3>
                <p className="mt-2 text-sm text-[var(--text-secondary)] max-w-[520px]">
                  Pick chains and protocols, generate a wallet, copy the
                  launch command. The runtime takes care of the rest.
                </p>
              </div>
              <div className="flex gap-3">
                <Button href="/onboard" variant="primary">
                  START ONBOARDING →
                </Button>
                <Button href="/docs" variant="ghost">
                  READ THE DOCS
                </Button>
              </div>
            </div>
          </Card>
        </div>
      </main>
      <Footer />
    </>
  )
}

function HostedSolverCard({ hosted }: { hosted: HostedSolver }) {
  const protocols = hosted.protocols.split(',').filter(Boolean)
  const chains = hosted.chains.split(',').filter(Boolean)
  const modeLabel = hosted.signing_mode === 'self_hosted' ? 'SELF-HOSTED'
    : hosted.signing_mode === 'remote_signer' ? 'REMOTE SIGNER'
    : 'SESSION KEY'
  return (
    <Link href={`/portal/${hosted.solver_id}`} className="block group">
      <Card
        padding="md"
        className="transition-all group-hover:border-[var(--brand-blue)]/40 group-hover:bg-[var(--bg-elevated)]"
      >
        <div className="flex items-start justify-between gap-4 flex-wrap">
          <div className="min-w-0">
            <div className="flex items-center gap-3 flex-wrap">
              <span className="text-[15px] text-[var(--text-primary)] tracking-[0.04em] font-mono">
                {hosted.name}
              </span>
              <Badge tone="neutral">
                {modeLabel}
              </Badge>
              <span className="font-mono text-[11px] text-[var(--text-tertiary)]">
                {hosted.evm_address.slice(0, 6)}…{hosted.evm_address.slice(-4)}
              </span>
            </div>
            <div className="mt-2.5 flex items-center gap-2 flex-wrap">
              {protocols.map((p) => {
                const color = protocolColors[p.toLowerCase()] ?? '#94B0C4'
                return (
                  <span key={p} className="text-[10px] font-mono tracking-[0.12em] uppercase px-2 py-0.5 rounded-[2px] border"
                    style={{ color, borderColor: `${color}30` }}>
                    {p}
                  </span>
                )
              })}
              {chains.length > 0 && (
                <>
                  <span className="text-[var(--text-tertiary)] text-[10px]">·</span>
                  <span className="text-[10px] text-[var(--text-tertiary)] font-mono tracking-[0.12em]">
                    {chains.join(' · ')}
                  </span>
                </>
              )}
            </div>
            <div className="mt-3 text-[11px] font-mono text-[var(--text-tertiary)] tracking-[0.08em]">
              registered{' '}
              <span className="text-[var(--text-secondary)]">
                {new Date(hosted.registered_at).toLocaleDateString()}
              </span>
              {hosted.donut_accrued_usd > 0 && (
                <> · donut <span className="text-[var(--solana-mint)]">${hosted.donut_accrued_usd.toFixed(4)}</span></>
              )}
            </div>
          </div>
          <div className="font-mono text-[var(--text-tertiary)] group-hover:text-[var(--brand-blue)] transition-colors">
            →
          </div>
        </div>
      </Card>
    </Link>
  )
}

interface SolverDisplay {
  address: string
  solanaAddress: string | null
  status: SolverStatus
  protocols: string[]
  chains: string[]
  pnl_total: number
  fills_total: number
  fills_24h: number
  last_fill_ts: string | null
  last_fill_protocol: string | null
  last_fill_tx: string | null
  last_fill_profit: number | null
}

function SolverCard({ solver, hostedId }: { solver: SolverDisplay; hostedId?: string }) {
  const statusMap = {
    live: { tone: 'mint' as const, label: 'LIVE', dot: true, pulse: true },
    offline: { tone: 'danger' as const, label: 'OFFLINE', dot: true, pulse: false },
    connecting: { tone: 'info' as const, label: 'CONNECTING', dot: true, pulse: true },
  }
  const s = statusMap[solver.status]
  // Prefer the solver_id from the hosted registry; fall back to first 8 hex chars of address
  const id = hostedId ?? solver.address.slice(2, 10).toLowerCase()

  return (
    <Link href={`/portal/${id}`} className="block group">
      <Card
        padding="md"
        className="transition-all group-hover:border-[var(--brand-blue)]/40 group-hover:bg-[var(--bg-elevated)]"
      >
        <div className="flex items-start justify-between gap-4 flex-wrap">
          <div className="min-w-0">
            <div className="flex items-center gap-3 flex-wrap">
              <span className="text-[15px] text-[var(--text-primary)] tracking-[0.04em] font-mono">
                {shortAddr(solver.address)}
              </span>
              <Badge tone={s.tone} dot={s.dot} pulse={s.pulse}>
                {s.label}
              </Badge>
            </div>
            <div className="mt-2.5 flex items-center gap-2 flex-wrap">
              {solver.protocols.length === 0 ? (
                <span className="text-[10px] text-[var(--text-tertiary)] font-mono tracking-[0.12em] uppercase">
                  No fills yet
                </span>
              ) : (
                solver.protocols.map((p) => {
                  const color = protocolColors[p.toLowerCase()] ?? '#94B0C4'
                  return (
                    <span
                      key={p}
                      className="text-[10px] font-mono tracking-[0.12em] uppercase px-2 py-0.5 rounded-[2px] border"
                      style={{
                        color,
                        borderColor: `${color}30`,
                      }}
                    >
                      {p}
                    </span>
                  )
                })
              )}
              {solver.chains.length > 0 && (
                <>
                  <span className="text-[var(--text-tertiary)] text-[10px]">·</span>
                  <span className="text-[10px] text-[var(--text-tertiary)] font-mono tracking-[0.12em]">
                    {solver.chains.join(' · ')}
                  </span>
                </>
              )}
            </div>
            <div className="mt-3 text-[11px] font-mono text-[var(--text-tertiary)] tracking-[0.08em]">
              last fill{' '}
              <span className="text-[var(--text-secondary)]">
                {fmtAge(solver.last_fill_ts)}
              </span>
              {solver.last_fill_protocol && (
                <>
                  {' '}· {solver.last_fill_protocol}
                </>
              )}
            </div>
          </div>

          <div className="grid grid-cols-3 gap-x-8 gap-y-2 shrink-0 min-w-[360px]">
            <Mini
              label="P&L"
              value={`${solver.pnl_total >= 0 ? '+' : ''}$${solver.pnl_total.toFixed(2)}`}
              tone={solver.pnl_total >= 0 ? 'mint' : 'danger'}
            />
            <Mini label="FILLS" value={solver.fills_total.toString()} tone="blue" />
            <Mini label="24H" value={solver.fills_24h.toString()} />
          </div>

          <div className="font-mono text-[var(--text-tertiary)] group-hover:text-[var(--brand-blue)] transition-colors">
            →
          </div>
        </div>
      </Card>
    </Link>
  )
}

function Mini({
  label,
  value,
  tone = 'default',
}: {
  label: string
  value: string
  tone?: 'default' | 'mint' | 'danger' | 'blue'
}) {
  const c = {
    default: 'text-[var(--text-primary)]',
    mint: 'text-[var(--solana-mint)]',
    danger: 'text-[var(--danger)]',
    blue: 'text-[var(--brand-blue)]',
  }
  return (
    <div className="flex flex-col gap-1">
      <span className="font-mono text-[9px] tracking-[0.24em] uppercase text-[var(--text-tertiary)]">
        {label}
      </span>
      <span className={`font-mono text-[14px] tabular-nums ${c[tone]}`}>{value}</span>
    </div>
  )
}
