'use client'

import Link from 'next/link'
import { useState } from 'react'
import { NavBar, Footer, Card, CardHeader, Button, Badge, StatTile } from '@/components/ui'
import { protocolColors } from '@/lib/tokens'

interface SolverRow {
  id: string
  name: string
  status: 'live' | 'paused' | 'failing' | 'spinning_up'
  protocols: string[]
  chains: string[]
  pnl_24h: number
  fills_24h: number
  latency_ms: number
  success_rate: number
  last_fill_ago: string
  operator: string
}

// Demo fleet — in production this'd be GET /api/solvers
const SOLVERS: SolverRow[] = [
  {
    id: 'b3e9a2',
    name: 'colosseum-prod-01',
    status: 'live',
    protocols: ['Across', 'deBridge', 'Mayan', 'LiFi'],
    chains: ['ETH', 'BASE', 'ARB', 'SOL'],
    pnl_24h: 142.83,
    fills_24h: 47,
    latency_ms: 121,
    success_rate: 0.96,
    last_fill_ago: '12s ago',
    operator: 'yawningmonsoon',
  },
  {
    id: '7f2c11',
    name: 'solana-only-shot',
    status: 'live',
    protocols: ['Mayan', 'deBridge', 'Wormhole'],
    chains: ['SOL', 'BASE', 'ARB'],
    pnl_24h: 38.92,
    fills_24h: 21,
    latency_ms: 87,
    success_rate: 0.91,
    last_fill_ago: '34s ago',
    operator: 'colosseum-team',
  },
  {
    id: 'a91d43',
    name: 'dry-run-perfectionist',
    status: 'paused',
    protocols: ['Across', 'Stargate'],
    chains: ['ETH', 'OP', 'BASE'],
    pnl_24h: 0,
    fills_24h: 0,
    latency_ms: 0,
    success_rate: 0,
    last_fill_ago: '2h ago',
    operator: 'maciej',
  },
  {
    id: 'c6e2f8',
    name: 'cctp-circle-lover',
    status: 'failing',
    protocols: ['CCTP', 'LiFi'],
    chains: ['ETH', 'BASE', 'AVAX'],
    pnl_24h: -1.12,
    fills_24h: 4,
    latency_ms: 412,
    success_rate: 0.5,
    last_fill_ago: '5m ago',
    operator: 'taifoon-bot',
  },
  {
    id: '8a44b1',
    name: 'evm-everything',
    status: 'spinning_up',
    protocols: ['Across', 'deBridge', 'LiFi', 'Stargate', 'Hop'],
    chains: ['ETH', 'BASE', 'ARB', 'OP', 'POL'],
    pnl_24h: 0,
    fills_24h: 0,
    latency_ms: 0,
    success_rate: 0,
    last_fill_ago: '—',
    operator: 'newcomer',
  },
]

type Filter = 'all' | 'live' | 'paused' | 'failing'

export default function PortalPage() {
  const [filter, setFilter] = useState<Filter>('all')
  const visible =
    filter === 'all' ? SOLVERS : SOLVERS.filter((s) => s.status === filter)

  const live = SOLVERS.filter((s) => s.status === 'live').length
  const totalPnl = SOLVERS.reduce((a, s) => a + s.pnl_24h, 0)
  const totalFills = SOLVERS.reduce((a, s) => a + s.fills_24h, 0)

  return (
    <>
      <NavBar />
      <main className="flex-1">
        {/* Header */}
        <div className="border-b border-[var(--border-subtle)]">
          <div className="max-w-[1400px] mx-auto px-6 py-8 flex items-end justify-between flex-wrap gap-4">
            <div>
              <h1 className="text-3xl font-bold tracking-tight">Solver portal</h1>
              <p className="mt-2 text-sm text-[var(--text-secondary)] max-w-[560px]">
                Spin up new solvers, monitor existing ones, pause or scale the fleet.
                Each solver runs as its own pod with its own wallet.
              </p>
            </div>
            <Button href="/onboard" variant="glow" size="lg">
              + Spin up new solver
            </Button>
          </div>

          <div className="max-w-[1400px] mx-auto px-6 pb-6 grid grid-cols-2 sm:grid-cols-4 gap-3">
            <StatTile label="Solvers in fleet" value={SOLVERS.length} />
            <StatTile label="Live now" value={live} tone="success" />
            <StatTile label="Fills (24h)" value={totalFills} tone="cyan" />
            <StatTile
              label="P&L (24h)"
              value={`$${totalPnl.toFixed(2)}`}
              tone={totalPnl >= 0 ? 'success' : 'danger'}
            />
          </div>
        </div>

        {/* Filters */}
        <div className="max-w-[1400px] mx-auto px-6 pt-6 flex items-center gap-2">
          {(['all', 'live', 'paused', 'failing'] as Filter[]).map((f) => (
            <button
              key={f}
              onClick={() => setFilter(f)}
              className={`h-8 px-3 rounded-[var(--r-md)] text-[12px] font-medium uppercase tracking-wider transition ${
                filter === f
                  ? 'bg-[var(--bg-elevated)] text-[var(--brand-cyan)] border border-[var(--brand-cyan)]/40'
                  : 'border border-[var(--border-default)] text-[var(--text-secondary)] hover:text-[var(--text-primary)]'
              }`}
            >
              {f}
            </button>
          ))}
        </div>

        {/* Solver list */}
        <div className="max-w-[1400px] mx-auto px-6 py-6 space-y-3">
          {visible.length === 0 && (
            <Card padding="lg" className="text-center">
              <div className="text-[var(--text-secondary)]">
                No solvers match this filter.
              </div>
            </Card>
          )}
          {visible.map((s) => (
            <SolverCard key={s.id} solver={s} />
          ))}
        </div>

        {/* Empty-state CTA card if user has no solvers (always visible at the end as a hint) */}
        <div className="max-w-[1400px] mx-auto px-6 pb-12">
          <Card padding="lg" glow>
            <div className="flex items-center justify-between flex-wrap gap-4">
              <div>
                <CardHeader title="Onboard another team / chain" />
                <p className="text-sm text-[var(--text-secondary)] max-w-[520px]">
                  Add a new solver in 3 steps. Pick chains and protocols, generate
                  a wallet, copy the launch command.
                </p>
              </div>
              <div className="flex gap-2">
                <Button href="/onboard" variant="primary">
                  Start onboarding
                </Button>
                <Button href="/docs" variant="ghost">
                  Read the docs
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

function SolverCard({ solver }: { solver: SolverRow }) {
  const statusMap = {
    live: { tone: 'success' as const, label: 'LIVE', dot: true, pulse: true },
    paused: { tone: 'warning' as const, label: 'PAUSED', dot: false, pulse: false },
    failing: { tone: 'danger' as const, label: 'FAILING', dot: true, pulse: true },
    spinning_up: { tone: 'info' as const, label: 'SPINNING UP', dot: true, pulse: true },
  }
  const s = statusMap[solver.status]

  return (
    <Link
      href={`/portal/${solver.id}`}
      className="block group"
    >
      <Card
        padding="md"
        className="transition-all group-hover:border-[var(--brand-cyan)]/40 group-hover:bg-[var(--bg-elevated)]"
      >
        <div className="flex items-start justify-between gap-4 flex-wrap">
          <div className="min-w-0">
            <div className="flex items-center gap-3 flex-wrap">
              <span className="font-bold text-[15px] text-[var(--text-primary)]">
                {solver.name}
              </span>
              <span className="font-mono text-[11px] text-[var(--text-tertiary)]">
                #{solver.id}
              </span>
              <Badge tone={s.tone} dot={s.dot} pulse={s.pulse}>
                {s.label}
              </Badge>
            </div>
            <div className="mt-2 flex items-center gap-2 flex-wrap">
              {solver.protocols.map((p) => {
                const color = protocolColors[p.toLowerCase()] ?? '#A0A0B0'
                return (
                  <span
                    key={p}
                    className="text-[10px] font-bold px-2 py-0.5 rounded-full"
                    style={{ color, border: `1px solid ${color}33`, background: `${color}11` }}
                  >
                    {p}
                  </span>
                )
              })}
              <span className="text-[var(--text-tertiary)] text-[10px]">·</span>
              <span className="text-[10px] text-[var(--text-tertiary)] font-mono">
                {solver.chains.join(' · ')}
              </span>
            </div>
            <div className="mt-2 text-[11px] text-[var(--text-tertiary)]">
              operator <span className="text-[var(--text-secondary)]">{solver.operator}</span> · last fill {solver.last_fill_ago}
            </div>
          </div>

          <div className="grid grid-cols-4 gap-3 shrink-0 min-w-[480px]">
            <Mini
              label="P&L 24h"
              value={`${solver.pnl_24h >= 0 ? '+' : ''}$${solver.pnl_24h.toFixed(2)}`}
              tone={solver.pnl_24h >= 0 ? 'success' : 'danger'}
            />
            <Mini label="Fills" value={solver.fills_24h.toString()} tone="cyan" />
            <Mini
              label="Latency"
              value={solver.status === 'live' ? `${solver.latency_ms}ms` : '—'}
              tone="violet"
            />
            <Mini
              label="Success"
              value={
                solver.status === 'live' || solver.status === 'failing'
                  ? `${(solver.success_rate * 100).toFixed(0)}%`
                  : '—'
              }
            />
          </div>

          <div className="text-[var(--text-tertiary)] group-hover:text-[var(--brand-cyan)] transition-colors text-xl">
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
  tone?: 'default' | 'success' | 'danger' | 'cyan' | 'violet'
}) {
  const c = {
    default: 'text-[var(--text-primary)]',
    success: 'text-[var(--success)]',
    danger: 'text-[var(--danger)]',
    cyan: 'text-[var(--brand-cyan)]',
    violet: 'text-[var(--brand-violet)]',
  }
  return (
    <div className="rounded-[var(--r-md)] bg-[var(--bg-raised)] border border-[var(--border-subtle)] px-3 py-2">
      <div className="text-[9px] uppercase tracking-[0.16em] text-[var(--text-tertiary)]">
        {label}
      </div>
      <div className={`mt-1 font-mono text-sm font-bold tabular-nums ${c[tone]}`}>
        {value}
      </div>
    </div>
  )
}
