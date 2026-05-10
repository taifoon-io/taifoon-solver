'use client'
import { useEffect, useState } from 'react'
import { protocolColors } from '@/lib/tokens'

/**
 * LiveTicker — a streaming list of fake fills that gives the hero its
 * "the runtime is alive" feel. Generates a new entry every ~1.6s,
 * keeps the last 6 visible, slides the oldest off the bottom.
 *
 * No actual API calls — this is a marketing decoration. The portal
 * uses the real SSE stream via useSolverEvents.
 *
 * Why fake data on the landing? Because the alternative is "still life"
 * — and the brand promise here is autonomy + speed. A blank panel
 * undersells what's behind the page.
 */

interface TickerRow {
  id: number
  ts: number
  proto: string
  src: string
  dst: string
  amt: string
  profit: number
  stage: 'confirmed' | 'broadcast' | 'calldata' | 'dry-run'
}

const PROTOS = [
  { name: 'Across V3', key: 'across' },
  { name: 'deBridge DLN', key: 'debridge' },
  { name: 'Mayan Swift', key: 'mayan' },
  { name: 'LiFi', key: 'lifi' },
  { name: 'Stargate V2', key: 'stargate' },
  { name: 'CCTP', key: 'cctp' },
]
const ROUTES = [
  ['ETH', 'ARB'],
  ['BASE', 'SOL'],
  ['SOL', 'BASE'],
  ['ARB', 'OP'],
  ['SOL', 'ARB'],
  ['POL', 'BASE'],
  ['ETH', 'BASE'],
  ['OP', 'BSC'],
]
const AMOUNTS = ['10K', '2.5K', '50K', '800', '120', '7.4K', '32K', '5K']
const STAGES = ['confirmed', 'broadcast', 'calldata', 'dry-run', 'confirmed', 'confirmed'] as const

const stageStyle = {
  confirmed: { color: '#14F195', label: 'CONFIRMED' },
  broadcast: { color: '#3DA5FF', label: 'BROADCAST' },
  calldata: { color: '#9945FF', label: 'CALLDATA' },
  'dry-run': { color: '#FFB454', label: 'DRY-RUN' },
}

function makeRow(id: number): TickerRow {
  const p = PROTOS[Math.floor(Math.random() * PROTOS.length)]
  const r = ROUTES[Math.floor(Math.random() * ROUTES.length)]
  const a = AMOUNTS[Math.floor(Math.random() * AMOUNTS.length)]
  const stage = STAGES[Math.floor(Math.random() * STAGES.length)]
  const profit =
    stage === 'dry-run'
      ? Math.random() * 3
      : stage === 'confirmed'
      ? 1 + Math.random() * 80
      : Math.random() * 30
  return {
    id,
    ts: Date.now(),
    proto: p.name,
    src: r[0],
    dst: r[1],
    amt: `${a} ${Math.random() > 0.4 ? 'USDC' : 'USDT'}`,
    profit,
    stage,
  }
}

export function LiveTicker() {
  const [rows, setRows] = useState<TickerRow[]>([])
  const [solverId, setSolverId] = useState<string>('taifoon')

  useEffect(() => {
    fetch('/api/hosting/solvers', { cache: 'no-store' })
      .then((r) => r.ok ? r.json() : null)
      .then((d) => {
        const first = d?.solvers?.[0]
        if (first?.solver_id) setSolverId(first.solver_id)
      })
      .catch(() => null)
  }, [])

  useEffect(() => {
    let id = 0
    // Defer the initial seed so it lands as a follow-up render rather
    // than a synchronous setState inside the effect (React 19 lint rule).
    queueMicrotask(() => {
      setRows([makeRow(id++), makeRow(id++), makeRow(id++)])
    })
    const interval = setInterval(() => {
      setRows((prev) => {
        const next = [makeRow(id++), ...prev].slice(0, 6)
        return next
      })
    }, 1600)
    return () => clearInterval(interval)
  }, [])

  return (
    <div className="rounded-[var(--r-sm)] border border-[var(--border-default)] bg-[var(--bg-elevated)] overflow-hidden backdrop-blur">
      <div className="flex items-center justify-between px-4 h-9 border-b border-[var(--border-subtle)]">
        <div className="flex items-center gap-3">
          <span className="w-1.5 h-1.5 rounded-full bg-[var(--solana-mint)] pulse-live" />
          <span className="font-mono text-[10px] tracking-[0.24em] uppercase text-[var(--solana-mint)]">
            live · genome stream
          </span>
        </div>
        <span className="font-mono text-[10px] tracking-[0.18em] uppercase text-[var(--text-tertiary)]">
          solver_{solverId}
        </span>
      </div>

      <div className="relative">
        {/* fade out the bottom row to suggest the stream continues */}
        <div className="pointer-events-none absolute inset-x-0 bottom-0 h-12 bg-gradient-to-t from-[var(--bg-elevated)] to-transparent z-10" />
        <ul className="flex flex-col">
          {rows.map((row, i) => (
            <Row key={row.id} row={row} index={i} />
          ))}
        </ul>
      </div>
    </div>
  )
}

function Row({ row, index }: { row: TickerRow; index: number }) {
  const key = row.proto.toLowerCase().split(' ')[0]
  const color = protocolColors[key] ?? '#94B0C4'
  const s = stageStyle[row.stage]
  const isNew = index === 0
  return (
    <li
      className="flex items-center gap-3 px-4 py-2.5 border-b border-[var(--border-subtle)] last:border-0"
      style={{
        background: `${color}05`,
        borderLeft: `2px solid ${color}`,
        animation: isNew ? 'ticker-in 500ms var(--ease-out) both' : undefined,
      }}
    >
      <span
        className="font-mono text-[10px] tracking-[0.12em] uppercase shrink-0"
        style={{ color }}
      >
        {row.proto}
      </span>
      <span className="font-mono text-[10px] text-[var(--text-tertiary)] tracking-[0.06em] shrink-0">
        {row.src} <span className="text-[var(--text-disabled)]">→</span> {row.dst}
      </span>
      <span className="font-mono text-[11px] text-[var(--text-secondary)] truncate flex-1">
        {row.amt}
      </span>
      <span
        className="font-mono text-[11px] tabular-nums shrink-0"
        style={{
          color: row.profit >= 0 ? 'var(--solana-mint)' : 'var(--text-tertiary)',
        }}
      >
        {row.profit >= 0 ? '+' : ''}${row.profit.toFixed(2)}
      </span>
      <span
        className="font-mono text-[9px] tracking-[0.18em] uppercase px-1.5 py-0.5 rounded-[2px] shrink-0"
        style={{ color: s.color, border: `1px solid ${s.color}40` }}
      >
        {s.label}
      </span>

      <style>{`
        @keyframes ticker-in {
          from { transform: translateY(-8px); opacity: 0; background-color: rgba(20,241,149,0.15); }
          to   { transform: translateY(0); opacity: 1; background-color: ${color}05; }
        }
      `}</style>
    </li>
  )
}
