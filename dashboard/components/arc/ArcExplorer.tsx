'use client'

/**
 * ArcExplorer — live cross-chain dispatch viewer on solver.taifoon.dev.
 *
 * Subscribes to arc-api's /v1/devnet/dispatches/sse and renders every
 * quote/fill/V5 review/devnet heartbeat/donut accrual as it lands.
 * Mirrors taifoon.io brand conventions used by the rest of this
 * dashboard: [ TAG ] labels, mono numbers, gap-px between cards,
 * pure-black canvas with rgba ink at opacity scales.
 *
 * The /arc page is the operator-facing surface — it sits next to
 * /watch (per-wallet view), /analytics (P&L), /portal (solver
 * registration). Use it to see what taifoon-arc is dispatching, who
 * the V5 reviewer approved, and which devnet RPC blinked.
 */

import { useMemo } from 'react'

import { Card, Tag, StatTile } from '@/components/ui'

import { useArcDispatchStream, type ArcDispatchEvent } from '@/lib/arc/sse'
import { OperatorOverlay } from './OperatorOverlay'

function timeAgo(ts: number): string {
  const now = Math.floor(Date.now() / 1000)
  const delta = Math.max(0, now - ts)
  if (delta < 60) return `${delta}s ago`
  if (delta < 3600) return `${Math.floor(delta / 60)}m ago`
  return `${Math.floor(delta / 3600)}h ago`
}

function short(s: string, head = 6, tail = 4): string {
  if (s.length <= head + tail + 1) return s
  return `${s.slice(0, head)}…${s.slice(-tail)}`
}

function chainLabel(id: number, kind?: string): string {
  if (kind === 'solana' || id === 0) return 'Solana devnet'
  switch (id) {
    case 1:      return 'Ethereum'
    case 10:     return 'Optimism'
    case 137:    return 'Polygon'
    case 8453:   return 'Base'
    case 42161:  return 'Arbitrum'
    case 56:     return 'BSC'
    case 59144:  return 'Linea'
    case 36927:  return 'Taifoon devnet'
    default:     return `Chain ${id}`
  }
}

function latencyTone(ms: number): 'mint' | 'warning' | 'danger' {
  if (ms < 200)  return 'mint'
  if (ms < 1000) return 'warning'
  return 'danger'
}

export function ArcExplorer() {
  const stream = useArcDispatchStream()

  const eventCounts = useMemo(() => stream.events.reduce<Record<string, number>>(
    (acc, e) => ({ ...acc, [e.kind]: (acc[e.kind] ?? 0) + 1 }), {},
  ), [stream.events])

  const lastFill = useMemo(
    () => stream.events.find((e) => e.kind === 'fill') as
      Extract<ArcDispatchEvent, { kind: 'fill' }> | undefined,
    [stream.events],
  )

  const donutTotals = useMemo(() => {
    const totals = { contributor: 0, treasury: 0, solver: 0, count: 0 }
    for (const e of stream.events) {
      if (e.kind !== 'donut_accrued') continue
      totals.contributor += e.contributor_usd
      totals.treasury    += e.treasury_usd
      totals.solver      += e.solver_usd
      totals.count       += 1
    }
    return totals
  }, [stream.events])

  return (
    <div className="relative">
      {/* ── Page header ─────────────────────────────────────────────── */}
      <section className="mx-auto max-w-6xl px-6 pt-10 pb-6 md:pt-14 md:pb-8">
        <div className="flex items-center gap-3 flex-wrap" data-testid="connection-banner">
          <span
            className="inline-block h-1.5 w-1.5 rounded-full animate-pulse"
            style={{
              background:
                stream.connection === 'open' ? 'var(--solana-mint)' :
                stream.connection === 'connecting' || stream.connection === 'init' ? 'var(--warning)' :
                'var(--danger)',
              boxShadow:
                stream.connection === 'open' ? '0 0 8px rgba(20,241,149,0.5)' :
                stream.connection === 'connecting' ? '0 0 6px rgba(255,180,84,0.5)' :
                'none',
            }}
          />
          <Tag tone="blue">Arc · Live dispatches</Tag>
          <span className="font-mono text-[10px] uppercase tracking-[0.25em]"
                style={{
                  color:
                    stream.connection === 'open' ? 'var(--solana-mint)' :
                    stream.connection === 'connecting' ? 'var(--warning)' :
                    stream.connection === 'error' ? 'var(--danger)' :
                    'var(--text-tertiary)',
                }}>
            {stream.connection === 'open' ? 'sse open' :
             stream.connection === 'connecting' ? 'connecting…' :
             stream.connection === 'error' ? `sse error · ${stream.reconnects} retries` :
             'init'}
          </span>
          {stream.endpoint && (
            <span className="font-mono text-[10px]" style={{ color: 'var(--text-tertiary)' }}>
              {stream.endpoint.replace(/^https?:\/\//, '')}
            </span>
          )}
          <a
            href="/arc/matrix"
            className="ml-auto font-mono text-[10px] uppercase tracking-[0.25em] underline-offset-4 hover:underline"
            style={{ color: 'var(--brand-blue)' }}
          >
            → matrix coverage
          </a>
        </div>
        <h1
          className="mt-6 text-3xl md:text-5xl font-light uppercase"
          style={{
            background: 'linear-gradient(180deg, var(--text-primary) 0%, rgba(230,240,247,0.5) 100%)',
            WebkitBackgroundClip: 'text',
            WebkitTextFillColor: 'transparent',
            letterSpacing: '0.08em',
          }}
        >
          Live dispatches
        </h1>
        <p className="mt-3 max-w-xl text-sm" style={{ color: 'var(--text-secondary)' }}>
          Every quote, fill, V5 review, and donut accrual flowing through
          taifoon-arc on taifoon-devnet (36927) + Solana devnet, in real time.
        </p>
      </section>

      {/* ── Devnet status row ───────────────────────────────────────── */}
      <section className="mx-auto max-w-6xl px-6 pb-px" data-testid="devnet-status">
        <div className="grid grid-cols-1 md:grid-cols-2 gap-px"
             style={{ background: 'var(--border-subtle)' }}>
          {Object.values(stream.latestTraces).map((t) => (
            <Card key={t.chain} padding="md">
              <div className="flex items-center gap-2">
                <span
                  className="inline-block h-1.5 w-1.5 rounded-full"
                  style={{
                    background: t.healthy ? 'var(--solana-mint)' : 'var(--danger)',
                    boxShadow: t.healthy ? '0 0 8px rgba(20,241,149,0.6)' : 'none',
                  }}
                />
                <Tag tone={t.healthy ? 'mint' : 'muted'}>{t.chain}</Tag>
              </div>
              <div className="mt-3 grid grid-cols-3 gap-3">
                <StatTile label="block"    value={t.block.toLocaleString()} />
                <StatTile label="latency"  value={`${t.latency_ms}ms`} tone={latencyTone(t.latency_ms)} />
                <StatTile label="chain id" value={t.chain_id ? String(t.chain_id) : '—'} />
              </div>
              <div className="mt-3 font-mono text-[11px]" style={{ color: 'var(--text-tertiary)' }}>
                {t.rpc_url}
              </div>
            </Card>
          ))}
          {Object.keys(stream.latestTraces).length === 0 && (
            <Card padding="md" className="md:col-span-2">
              <Tag tone="muted">
                {stream.connection === 'open'
                  ? 'sse connected · awaiting first probe (≤10s)'
                  : stream.connection === 'connecting' || stream.connection === 'init'
                    ? 'connecting to arc-api…'
                    : `cannot reach arc-api · ${stream.endpoint || '?'}`}
              </Tag>
              {stream.connection === 'error' && (
                <p className="mt-2 text-[11px]" style={{ color: 'var(--text-tertiary)' }}>
                  Set <code className="font-mono">NEXT_PUBLIC_ARC_API_URL</code> at build time, or
                  expose <code className="font-mono">arc.taifoon.dev</code> with CORS allowing this origin.
                </p>
              )}
            </Card>
          )}
        </div>
      </section>

      {/* ── Stats row ───────────────────────────────────────────────── */}
      <section className="mx-auto max-w-6xl px-6 pb-px" data-testid="stats-row">
        <div className="grid grid-cols-2 md:grid-cols-5 gap-px"
             style={{ background: 'var(--border-subtle)' }}>
          <Card padding="md"><StatTile label="events"     value={stream.events.length} /></Card>
          <Card padding="md"><StatTile label="fills"      value={eventCounts.fill      ?? 0} tone="mint" /></Card>
          <Card padding="md"><StatTile label="reviews"    value={eventCounts.box_review ?? 0} tone="blue" /></Card>
          <Card padding="md"><StatTile label="quotes"     value={eventCounts.quote     ?? 0} /></Card>
          <Card padding="md"><StatTile label="reconnects" value={stream.reconnects} tone={stream.reconnects > 0 ? 'warning' : 'default'} /></Card>
        </div>
      </section>

      {/* ── Donut accrual summary ───────────────────────────────────── */}
      <section className="mx-auto max-w-6xl px-6 pb-px" data-testid="donut-summary">
        <Card padding="md">
          <div className="flex items-center justify-between mb-4">
            <Tag tone="blue">Donut · 30% surcharge on protocol fee</Tag>
            <span className="font-mono text-xs" style={{ color: 'var(--text-tertiary)' }}>
              {donutTotals.count} accruals this session
            </span>
          </div>
          <div className="grid grid-cols-1 md:grid-cols-3 gap-px"
               style={{ background: 'var(--border-subtle)' }}>
            <Card padding="md">
              <StatTile label="contributor · 70%" value={`$${donutTotals.contributor.toFixed(4)}`} tone="blue" />
            </Card>
            <Card padding="md">
              <StatTile label="treasury · 20%"    value={`$${donutTotals.treasury.toFixed(4)}`} />
            </Card>
            <Card padding="md">
              <StatTile label="solver · 10%"      value={`$${donutTotals.solver.toFixed(4)}`} tone="mint" />
            </Card>
          </div>
        </Card>
      </section>

      {/* ── Last fill spotlight ─────────────────────────────────────── */}
      {lastFill && (
        <section className="mx-auto max-w-6xl px-6 pb-px" data-testid="last-fill">
          <Card padding="md">
            <Tag tone="mint">{`Last fill · ${timeAgo(lastFill.ts)}`}</Tag>
            <div className="mt-3 flex flex-wrap items-baseline gap-x-4 gap-y-2 font-mono text-sm">
              <span style={{ color: 'var(--text-primary)' }}>{lastFill.protocol_slug}</span>
              <span style={{ color: 'var(--text-tertiary)' }}>·</span>
              <span style={{ color: 'var(--brand-blue)' }}>
                {chainLabel(lastFill.src_chain_id)} → {chainLabel(lastFill.dst_chain_id, lastFill.dst_kind)}
              </span>
              <span style={{ color: 'var(--text-tertiary)' }}>·</span>
              <span>${lastFill.value_routed_usd.toFixed(2)} routed</span>
              <span style={{ color: 'var(--text-tertiary)' }}>·</span>
              <span style={{ color: 'var(--solana-mint)' }}>+${lastFill.donut_fee_usd.toFixed(4)} donut</span>
              <span style={{ color: 'var(--text-tertiary)' }}>·</span>
              <span style={{ color: 'var(--text-secondary)' }}>{lastFill.status}</span>
            </div>
            {lastFill.fill_tx && (
              <div className="mt-2 font-mono text-xs" style={{ color: 'var(--text-tertiary)' }}>
                tx · {short(lastFill.fill_tx, 10, 8)}
              </div>
            )}
          </Card>
        </section>
      )}

      {/* ── Dispatch stream ─────────────────────────────────────────── */}
      <section className="mx-auto max-w-6xl px-6 pb-12">
        <Card padding="md">
          <div className="flex items-center justify-between mb-3">
            <Tag tone="blue">{`Stream · live (last ${stream.events.length})`}</Tag>
            <Tag tone="muted">↓ newest</Tag>
          </div>
          {stream.events.length === 0 ? (
            <div className="py-12 text-center">
              <Tag tone="muted">No events yet — POST /v1/fill on arc-api to start the stream</Tag>
            </div>
          ) : (
            <ul data-testid="dispatch-stream"
                className="divide-y"
                style={{ borderColor: 'var(--border-subtle)' }}>
              {stream.events.map((e, idx) => (
                <li key={idx} className="py-3">
                  <EventRow ev={e} />
                </li>
              ))}
            </ul>
          )}
        </Card>
      </section>

      <OperatorOverlay />
    </div>
  )
}

function EventRow({ ev }: { ev: ArcDispatchEvent }) {
  const ts = (
    <span className="font-mono text-[11px] w-16 shrink-0" style={{ color: 'var(--text-tertiary)' }}>
      {timeAgo(ev.ts)}
    </span>
  )

  if (ev.kind === 'fill') {
    return (
      <div className="flex items-baseline gap-3 flex-wrap">
        {ts}
        <Tag tone="mint">fill</Tag>
        <span className="font-mono text-sm" style={{ color: 'var(--text-primary)' }}>{ev.protocol_slug}</span>
        <span className="font-mono text-xs" style={{ color: 'var(--text-secondary)' }}>
          {chainLabel(ev.src_chain_id)} → {chainLabel(ev.dst_chain_id, ev.dst_kind)}
        </span>
        <span className="ml-auto font-mono text-xs" style={{ color: 'var(--solana-mint)' }}>
          ${ev.value_routed_usd.toFixed(2)} · +${ev.donut_fee_usd.toFixed(4)}
        </span>
      </div>
    )
  }

  if (ev.kind === 'quote') {
    return (
      <div className="flex items-baseline gap-3 flex-wrap">
        {ts}
        <Tag tone="blue">quote</Tag>
        <span className="font-mono text-sm">{ev.protocol_slug}</span>
        <span className="font-mono text-xs" style={{ color: 'var(--text-secondary)' }}>
          {chainLabel(ev.src_chain_id)} → {chainLabel(ev.dst_chain_id, ev.dst_kind)}
        </span>
        <span className="ml-auto font-mono text-xs" style={{ color: 'var(--text-secondary)' }}>
          fee ${ev.protocol_fee_usd.toFixed(4)} · +${ev.donut_surcharge.toFixed(4)} · ~{ev.estimated_gas.toLocaleString()} gas
        </span>
      </div>
    )
  }

  if (ev.kind === 'box_review') {
    const tone: 'mint' | 'blue' | 'muted' = ev.approved ? 'mint' : ev.dry_run ? 'blue' : 'muted'
    return (
      <div className="flex items-baseline gap-3 flex-wrap">
        {ts}
        <Tag tone={tone}>{`v5 · ${ev.verdict}`}</Tag>
        <span className="font-mono text-sm">{ev.protocol_slug}</span>
        <span className="font-mono text-xs" style={{ color: 'var(--text-secondary)' }}>
          {short(ev.intent_id, 8, 4)}
        </span>
        {ev.dry_run && <Tag tone="muted">dry-run</Tag>}
      </div>
    )
  }

  if (ev.kind === 'devnet_trace') {
    return (
      <div className="flex items-baseline gap-3 flex-wrap">
        {ts}
        <Tag tone={ev.healthy ? 'mint' : 'muted'}>heartbeat</Tag>
        <span className="font-mono text-sm">{ev.chain}</span>
        <span className="font-mono text-xs" style={{ color: 'var(--text-secondary)' }}>
          block {ev.block.toLocaleString()}
        </span>
        <span
          className="ml-auto font-mono text-xs"
          style={{ color: ev.latency_ms < 200 ? 'var(--solana-mint)' : 'var(--warning)' }}
        >
          {ev.latency_ms}ms
        </span>
      </div>
    )
  }

  if (ev.kind === 'donut_accrued') {
    return (
      <div className="flex items-baseline gap-3 flex-wrap">
        {ts}
        <Tag tone="blue">donut</Tag>
        <span className="font-mono text-sm">{ev.protocol_slug}</span>
        <span className="font-mono text-xs" style={{ color: 'var(--text-secondary)' }}>
          fee ${ev.protocol_fee_usd.toFixed(4)} → +${ev.surcharge_usd.toFixed(4)}
        </span>
        <span className="ml-auto font-mono text-xs" style={{ color: 'var(--text-secondary)' }}>
          C ${ev.contributor_usd.toFixed(4)} · T ${ev.treasury_usd.toFixed(4)} · S ${ev.solver_usd.toFixed(4)}
        </span>
      </div>
    )
  }

  return null
}
