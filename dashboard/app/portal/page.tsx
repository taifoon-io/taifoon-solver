'use client'

import Link from 'next/link'
import { useState } from 'react'
import { NavBar, Footer, Card, Button, Badge, StatTile, Tag } from '@/components/ui'
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
          <div className="max-w-[1400px] mx-auto px-6 py-12 flex items-end justify-between flex-wrap gap-6">
            <div>
              <Tag>The portal</Tag>
              <h1 className="tf-display tf-gradient-silver mt-4 text-[clamp(2rem,4vw,3rem)]">
                Solver fleet.
              </h1>
              <p className="mt-3 text-sm text-[var(--text-secondary)] max-w-[560px] leading-relaxed">
                Spin up new solvers, monitor existing ones, pause or scale
                the fleet. Each solver runs as its own pod with its own
                wallet — Taifoon&apos;s grid keeps the proofs.
              </p>
            </div>
            <Button href="/onboard" variant="primary" size="lg">
              <span className="text-[var(--text-tertiary)]">+</span>
              SPIN UP NEW SOLVER →
            </Button>
          </div>

          <div className="max-w-[1400px] mx-auto px-6 pb-10 grid grid-cols-2 sm:grid-cols-4 gap-x-12 gap-y-6">
            <StatTile label="FLEET" value={SOLVERS.length} />
            <StatTile label="LIVE" value={live} tone="mint" />
            <StatTile label="FILLS / 24H" value={totalFills} tone="blue" />
            <StatTile
              label="P&L / 24H"
              value={`$${totalPnl.toFixed(2)}`}
              tone={totalPnl >= 0 ? 'mint' : 'danger'}
            />
          </div>
        </div>

        {/* Filters */}
        <div className="max-w-[1400px] mx-auto px-6 pt-8 flex items-center gap-4">
          {(['all', 'live', 'paused', 'failing'] as Filter[]).map((f) => (
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

function SolverCard({ solver }: { solver: SolverRow }) {
  const statusMap = {
    live: { tone: 'mint' as const, label: 'LIVE', dot: true, pulse: true },
    paused: { tone: 'warning' as const, label: 'PAUSED', dot: false, pulse: false },
    failing: { tone: 'danger' as const, label: 'FAILING', dot: true, pulse: true },
    spinning_up: { tone: 'info' as const, label: 'SPINNING UP', dot: true, pulse: true },
  }
  const s = statusMap[solver.status]

  return (
    <Link href={`/portal/${solver.id}`} className="block group">
      <Card
        padding="md"
        className="transition-all group-hover:border-[var(--brand-blue)]/40 group-hover:bg-[var(--bg-elevated)]"
      >
        <div className="flex items-start justify-between gap-4 flex-wrap">
          <div className="min-w-0">
            <div className="flex items-center gap-3 flex-wrap">
              <span className="text-[15px] text-[var(--text-primary)] tracking-[0.04em]">
                {solver.name}
              </span>
              <span className="font-mono text-[10px] tracking-[0.16em] text-[var(--text-tertiary)]">
                #{solver.id}
              </span>
              <Badge tone={s.tone} dot={s.dot} pulse={s.pulse}>
                {s.label}
              </Badge>
            </div>
            <div className="mt-2.5 flex items-center gap-2 flex-wrap">
              {solver.protocols.map((p) => {
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
              })}
              <span className="text-[var(--text-tertiary)] text-[10px]">·</span>
              <span className="text-[10px] text-[var(--text-tertiary)] font-mono tracking-[0.12em]">
                {solver.chains.join(' · ')}
              </span>
            </div>
            <div className="mt-3 text-[11px] font-mono text-[var(--text-tertiary)] tracking-[0.08em]">
              operator{' '}
              <span className="text-[var(--text-secondary)]">{solver.operator}</span> · last fill{' '}
              {solver.last_fill_ago}
            </div>
          </div>

          <div className="grid grid-cols-4 gap-x-8 gap-y-2 shrink-0 min-w-[480px]">
            <Mini
              label="P&L 24H"
              value={`${solver.pnl_24h >= 0 ? '+' : ''}$${solver.pnl_24h.toFixed(2)}`}
              tone={solver.pnl_24h >= 0 ? 'mint' : 'danger'}
            />
            <Mini label="FILLS" value={solver.fills_24h.toString()} tone="blue" />
            <Mini
              label="LATENCY"
              value={solver.status === 'live' ? `${solver.latency_ms}ms` : '—'}
            />
            <Mini
              label="SUCCESS"
              value={
                solver.status === 'live' || solver.status === 'failing'
                  ? `${(solver.success_rate * 100).toFixed(0)}%`
                  : '—'
              }
            />
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
