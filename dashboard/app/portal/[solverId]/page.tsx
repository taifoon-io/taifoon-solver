'use client'

import { use, useEffect, useState } from 'react'
import Link from 'next/link'
import {
  useSolverEvents,
  protocolColor,
  protocolLabel,
  chainName,
  stageLabel,
  stageColor,
  LambdaStage,
  Intent,
  ProtoStats,
  LiveEvent,
} from '@/hooks/useSolverEvents'
import { NavBar, Footer, Card, CardHeader, Badge, StatTile, Button, Tag } from '@/components/ui'

// ── Chain badge ──────────────────────────────────────────────────────────────
function Chain({ id }: { id: number }) {
  return (
    <span className="text-[10px] font-mono px-1.5 py-0.5 rounded bg-[var(--bg-raised)] text-[var(--text-secondary)]">
      {chainName(id)}
    </span>
  )
}

function ProtoPill({ proto }: { proto: string }) {
  const color = protocolColor(proto)
  return (
    <span
      className="text-[10px] font-bold px-2 py-0.5 rounded-full"
      style={{ color, border: `1px solid ${color}33`, background: `${color}11` }}
    >
      {protocolLabel(proto)}
    </span>
  )
}

function StageBadge({ stage }: { stage: LambdaStage }) {
  const color = stageColor(stage)
  const dot = stage === 'broadcast' || stage === 'pending_confirmation'
  return (
    <span
      className="inline-flex items-center gap-1 text-[10px] font-mono px-1.5 py-0.5 rounded"
      style={{ color, background: `${color}1a` }}
    >
      {dot && <span className="w-1.5 h-1.5 rounded-full animate-pulse" style={{ background: color }} />}
      {stageLabel(stage)}
    </span>
  )
}

const STAGES: LambdaStage[] = [
  'detected',
  'profitability_check',
  'calldata_build',
  'estimate_gate',
  'broadcast',
  'confirmed',
]

function LifecycleBar({ stage }: { stage: LambdaStage }) {
  const activeIdx = STAGES.indexOf(stage)
  const isTerminal =
    stage === 'skipped' || stage === 'failed' || stage === 'reverted' || stage === 'dry_run'
  return (
    <div className="flex items-center gap-0.5 mt-1.5">
      {STAGES.map((s, i) => {
        const reached = !isTerminal && activeIdx >= i
        const isCurrent = !isTerminal && activeIdx === i
        return (
          <div key={s} className="flex items-center gap-0.5">
            <div
              className="h-1 rounded-full transition-all duration-300"
              style={{
                width: i === 0 ? 8 : 20,
                background: isCurrent
                  ? stageColor(s)
                  : reached
                    ? '#00FF8888'
                    : 'var(--border-default)',
              }}
            />
          </div>
        )
      })}
      {isTerminal && (
        <span className="ml-1.5">
          <StageBadge stage={stage} />
        </span>
      )}
    </div>
  )
}

function IntentRow({ intent }: { intent: Intent }) {
  const color = protocolColor(intent.protocol)
  const isGreen = intent.stage === 'confirmed'
  const isDry = intent.stage === 'dry_run'
  const isSkip = intent.stage === 'skipped' || intent.stage === 'failed' || intent.stage === 'reverted'

  const amtNum = parseFloat(intent.amount) || 0
  const decimals = amtNum > 1e15 ? 18 : amtNum > 1e5 ? 6 : 0
  const amtDisplay = decimals > 0 ? (amtNum / Math.pow(10, decimals)).toFixed(2) : amtNum.toFixed(2)

  return (
    <div
      className={`border-l-2 pl-3 py-2 rounded-r transition-all ${
        isGreen
          ? 'bg-[#00FF8810]'
          : isDry
            ? 'bg-[#FFB80012]'
            : isSkip
              ? 'opacity-50'
              : 'bg-[var(--bg-raised)]'
      }`}
      style={{ borderLeftColor: color }}
    >
      <div className="flex items-center justify-between gap-2 flex-wrap">
        <div className="flex items-center gap-2 min-w-0">
          <ProtoPill proto={intent.protocol} />
          <Chain id={intent.src_chain} />
          <span className="text-[var(--text-tertiary)] text-xs">→</span>
          <Chain id={intent.dst_chain} />
          <span className="font-mono text-xs text-[var(--text-secondary)]">{amtDisplay}</span>
        </div>
        <div className="flex items-center gap-2">
          {intent.profit_usd !== undefined && intent.profit_usd !== null && (
            <span
              className={`font-mono text-xs ${
                intent.profit_usd > 0 ? 'text-[var(--success)]' : 'text-[var(--text-tertiary)]'
              }`}
            >
              ${intent.profit_usd.toFixed(2)}
            </span>
          )}
          {intent.tx_hash && (
            <span className="font-mono text-[10px] text-[var(--brand-cyan)]">{intent.tx_hash.slice(0, 10)}…</span>
          )}
          <StageBadge stage={intent.stage} />
        </div>
      </div>
      <LifecycleBar stage={intent.stage} />
    </div>
  )
}

const KNOWN_PROTOCOLS = ['across_v3', 'debridge_dln', 'mayan_swift', 'lifi', 'orbiter_finance']
const PROTO_NAMES: Record<string, string> = {
  across_v3: 'Across V3',
  debridge_dln: 'deBridge DLN',
  mayan_swift: 'Mayan Swift',
  lifi: 'LiFi',
  orbiter_finance: 'Orbiter',
}

function ProtocolPanel({ protocols }: { protocols: Record<string, ProtoStats> }) {
  // Tick every 5s so "active within 30s" stays correct as time passes,
  // without calling Date.now() during render (impure). The interval IS the
  // external system being synchronized to React state.
  const [now, setNow] = useState(0)
  useEffect(() => {
    const id = setInterval(() => setNow(Date.now()), 5_000)
    queueMicrotask(() => setNow(Date.now()))
    return () => clearInterval(id)
  }, [])

  return (
    <Card padding="md">
      <CardHeader title="5 Protocols" />
      <div className="space-y-3">
        {KNOWN_PROTOCOLS.map((key) => {
          const stat = Object.entries(protocols).find(([k]) => k.includes(key.split('_')[0]))
          const data = stat?.[1]
          const color = protocolColor(key)
          const active = !!data && now > 0 && now - data.last_ms < 30_000
          return (
            <div key={key} className="flex items-center justify-between">
              <div className="flex items-center gap-2">
                <div
                  className={`w-1.5 h-1.5 rounded-full ${active ? 'animate-pulse' : 'opacity-30'}`}
                  style={{ background: color }}
                />
                <span className="text-xs font-medium" style={{ color: active ? color : 'var(--text-tertiary)' }}>
                  {PROTO_NAMES[key]}
                </span>
              </div>
              {data ? (
                <div className="flex items-center gap-2 text-[10px] font-mono">
                  <span className="text-[var(--text-tertiary)]">{data.seen} seen</span>
                  {data.dry_run > 0 && <span className="text-[var(--warning)]">{data.dry_run} dry</span>}
                  {data.confirmed > 0 && <span className="text-[var(--success)]">{data.confirmed} ✓</span>}
                  {data.skipped > 0 && <span className="text-[var(--text-tertiary)]">{data.skipped} skip</span>}
                </div>
              ) : (
                <span className="text-[10px] text-[var(--text-tertiary)] font-mono">waiting…</span>
              )}
            </div>
          )
        })}
      </div>
    </Card>
  )
}

function EventTicker({ events }: { events: LiveEvent[] }) {
  return (
    <Card padding="md">
      <CardHeader title="Event Stream" />
      <div className="overflow-y-auto space-y-1.5 pr-1" style={{ maxHeight: 340 }}>
        {events.length === 0 && (
          <div className="text-[var(--text-tertiary)] text-xs text-center py-8">No events yet…</div>
        )}
        {events.map((e, i) => (
          <div key={i} className="flex items-start gap-2 text-[11px]">
            <span className="text-[var(--text-tertiary)] font-mono shrink-0">
              {new Date(e.ts).toLocaleTimeString()}
            </span>
            <div className="flex items-center gap-1.5 flex-wrap">
              {e.protocol && (
                <span className="font-bold" style={{ color: protocolColor(e.protocol) }}>
                  {protocolLabel(e.protocol)}
                </span>
              )}
              <span
                className={
                  e.type === 'solved'
                    ? 'text-[var(--success)]'
                    : e.type === 'failed'
                      ? 'text-[var(--danger)]'
                      : e.stage === 'dry_run'
                        ? 'text-[var(--warning)]'
                        : 'text-[var(--text-secondary)]'
                }
              >
                {e.detail}
              </span>
              {e.tx_hash && (
                <span className="font-mono text-[var(--brand-cyan)]">{e.tx_hash.slice(0, 8)}…</span>
              )}
            </div>
          </div>
        ))}
      </div>
    </Card>
  )
}

interface PageProps {
  params: Promise<{ solverId: string }>
}

export default function SolverMonitorPage({ params }: PageProps) {
  const { solverId } = use(params)
  const { intents, stats, protocols, events, logs, connected } = useSolverEvents()

  const dryRuns = intents.filter((i) => i.stage === 'dry_run').length
  const confirmed = intents.filter((i) => i.stage === 'confirmed').length
  const skipped = intents.filter((i) => i.stage === 'skipped').length

  return (
    <>
      <NavBar />
      <main className="flex-1">
        {/* Solver header */}
        <div className="border-b border-[var(--border-subtle)] bg-[var(--bg-elevated)]">
          <div className="max-w-[1400px] mx-auto px-6 py-5 flex items-center justify-between flex-wrap gap-3">
            <div className="flex items-center gap-4">
              <Link
                href="/portal"
                className="font-mono text-[11px] tracking-[0.2em] uppercase text-[var(--text-tertiary)] hover:text-[var(--brand-blue)] transition-colors"
              >
                ← PORTAL
              </Link>
              <span className="text-[var(--border-default)]">/</span>
              <span className="font-mono text-[13px] tracking-[0.08em] text-[var(--text-primary)]">
                solver_<span className="text-[var(--brand-blue)]">{solverId}</span>
              </span>
              <Badge tone={connected ? 'mint' : 'neutral'} dot pulse={connected}>
                {connected ? 'LIVE' : 'CONNECTING'}
              </Badge>
              <Badge tone="info">5 PROTOCOLS</Badge>
            </div>
            <div className="flex items-center gap-4">
              <div className="flex items-baseline gap-2">
                <span className="font-mono text-[10px] tracking-[0.2em] uppercase text-[var(--text-tertiary)]">
                  P&amp;L today
                </span>
                <span className="font-mono text-[14px] text-[var(--solana-mint)]">
                  ${(stats?.net_profit_today_usd ?? 0).toFixed(4)}
                </span>
              </div>
              <Button variant="secondary" size="sm">
                PAUSE
              </Button>
              <Button variant="ghost" size="sm">
                LOGS
              </Button>
            </div>
          </div>
        </div>

        {/* Stats row */}
        <div className="max-w-[1400px] mx-auto grid grid-cols-2 sm:grid-cols-4 lg:grid-cols-8 gap-x-8 gap-y-4 px-6 py-6 border-b border-[var(--border-subtle)]">
          <StatTile label="INTENTS" value={stats?.total_intents ?? intents.length} />
          <StatTile label="DRY RUNS" value={dryRuns} tone="warning" />
          <StatTile label="CONFIRMED" value={confirmed} tone="mint" />
          <StatTile label="SKIPPED" value={skipped} />
          <StatTile label="FILLS" value={stats?.executed_fills ?? 0} tone="mint" />
          <StatTile label="FAILED" value={stats?.failed_fills ?? 0} tone="danger" />
          <StatTile
            label="SUCCESS"
            value={`${((stats?.success_rate ?? 0) * 100).toFixed(0)}%`}
            tone="blue"
          />
          <StatTile label="LATENCY" value={`${stats?.latency_ms ?? 0}`} unit="ms" tone="blue" />
        </div>

        {/* Main content */}
        <div className="max-w-[1400px] mx-auto grid grid-cols-1 lg:grid-cols-3 gap-3 px-6 pb-12">
          <div className="lg:col-span-2 space-y-3">
            <Card padding="sm">
              <div className="px-1 mb-2">
                <Tag>Lambda lifecycle</Tag>
              </div>
              <div className="flex flex-wrap gap-2">
                {(
                  [
                    'detected',
                    'profitability_check',
                    'calldata_build',
                    'estimate_gate',
                    'broadcast',
                    'confirmed',
                    'dry_run',
                    'skipped',
                    'failed',
                  ] as LambdaStage[]
                ).map((s) => (
                  <StageBadge key={s} stage={s} />
                ))}
              </div>
            </Card>

            <Card padding="none">
              <div className="flex items-center justify-between px-4 py-3 border-b border-[var(--border-subtle)]">
                <Tag>Intent stream</Tag>
                <span className="font-mono text-[10px] tracking-[0.16em] text-[var(--text-tertiary)]">
                  {intents.length} tracked
                </span>
              </div>
              <div className="divide-y divide-[var(--border-subtle)] max-h-[640px] overflow-y-auto px-3 py-1">
                {intents.length === 0 && (
                  <div className="text-[var(--text-tertiary)] text-xs text-center py-12">
                    Waiting for intents…
                  </div>
                )}
                {intents.map((intent) => (
                  <div key={intent.id} className="py-1.5">
                    <IntentRow intent={intent} />
                  </div>
                ))}
              </div>
            </Card>
          </div>

          <div className="space-y-3">
            <ProtocolPanel protocols={protocols} />
            <EventTicker events={events} />

            <Card padding="md">
              <CardHeader title="Stage Breakdown" />
              {(
                [
                  'detected',
                  'calldata_build',
                  'dry_run',
                  'confirmed',
                  'skipped',
                  'failed',
                  'reverted',
                ] as LambdaStage[]
              ).map((stage) => {
                const count = intents.filter((i) => i.stage === stage).length
                const pct = intents.length > 0 ? (count / intents.length) * 100 : 0
                return (
                  <div key={stage} className="mb-2">
                    <div className="flex justify-between text-[11px] mb-0.5">
                      <span style={{ color: stageColor(stage) }}>{stageLabel(stage)}</span>
                      <span className="font-mono text-[var(--text-tertiary)]">{count}</span>
                    </div>
                    <div className="h-1 bg-[var(--bg-raised)] rounded-full overflow-hidden">
                      <div
                        className="h-full rounded-full transition-all duration-500"
                        style={{ width: `${pct}%`, background: stageColor(stage) }}
                      />
                    </div>
                  </div>
                )
              })}
            </Card>

            <Card padding="md">
              <CardHeader title="Solver Logs" />
              <div
                className="overflow-y-auto space-y-0.5 font-mono text-[10px] leading-relaxed"
                style={{ maxHeight: 280 }}
              >
                {logs.length === 0 && (
                  <div className="text-[var(--text-tertiary)] text-center py-6">No logs yet…</div>
                )}
                {logs.map((line, i) => {
                  const isErr = line.includes('ERROR') || line.includes('❌')
                  const isWarn = line.includes('WARN') || line.includes('⚠')
                  const isOk = line.includes('✅') || line.includes('🎉') || line.includes('confirmed')
                  const isDry = line.includes('DRY_RUN') || line.includes('🧪')
                  return (
                    <div
                      key={i}
                      className={
                        isErr
                          ? 'text-[var(--danger)]'
                          : isWarn
                            ? 'text-[var(--warning)]'
                            : isOk
                              ? 'text-[var(--success)]'
                              : isDry
                                ? 'text-[var(--warning)]'
                                : 'text-[var(--text-tertiary)]'
                      }
                    >
                      {line}
                    </div>
                  )
                })}
              </div>
            </Card>
          </div>
        </div>
      </main>
      <Footer />
    </>
  )
}
