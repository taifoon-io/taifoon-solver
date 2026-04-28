'use client'

import { useSolverEvents, protocolColor, protocolLabel, chainName, stageLabel, stageColor, LambdaStage, Intent, ProtoStats, LiveEvent } from '@/hooks/useSolverEvents'

// ── Chain badge ──────────────────────────────────────────────────────────────
function Chain({ id }: { id: number }) {
  return <span className="text-[10px] font-mono px-1.5 py-0.5 rounded bg-gray-800 text-gray-300">{chainName(id)}</span>
}

// ── Protocol pill ────────────────────────────────────────────────────────────
function ProtoPill({ proto }: { proto: string }) {
  const color = protocolColor(proto)
  return (
    <span className="text-[10px] font-bold px-2 py-0.5 rounded-full" style={{ color, border: `1px solid ${color}33`, background: `${color}11` }}>
      {protocolLabel(proto)}
    </span>
  )
}

// ── Lambda stage badge ───────────────────────────────────────────────────────
function StageBadge({ stage }: { stage: LambdaStage }) {
  const color = stageColor(stage)
  const dot = stage === 'broadcast' || stage === 'pending_confirmation'
  return (
    <span className="inline-flex items-center gap-1 text-[10px] font-mono px-1.5 py-0.5 rounded"
      style={{ color, background: `${color}1a` }}>
      {dot && <span className="w-1.5 h-1.5 rounded-full animate-pulse" style={{ background: color }} />}
      {stageLabel(stage)}
    </span>
  )
}

// ── Lambda lifecycle pipeline ────────────────────────────────────────────────
const STAGES: LambdaStage[] = [
  'detected', 'profitability_check', 'calldata_build',
  'estimate_gate', 'broadcast', 'confirmed',
]

function LifecycleBar({ stage }: { stage: LambdaStage }) {
  const activeIdx = STAGES.indexOf(stage)
  const isTerminal = stage === 'skipped' || stage === 'failed' || stage === 'reverted' || stage === 'dry_run'
  return (
    <div className="flex items-center gap-0.5 mt-1.5">
      {STAGES.map((s, i) => {
        const reached = !isTerminal && activeIdx >= i
        const isCurrent = !isTerminal && activeIdx === i
        const color = reached ? stageColor('confirmed') : '#1E293B'
        return (
          <div key={s} className="flex items-center gap-0.5">
            <div className="h-1 rounded-full transition-all duration-300"
              style={{ width: i === 0 ? 8 : 20, background: isCurrent ? stageColor(s) : reached ? '#00FF8888' : '#1E293B' }} />
          </div>
        )
      })}
      {isTerminal && (
        <span className="ml-1.5"><StageBadge stage={stage} /></span>
      )}
    </div>
  )
}

// ── Single intent row ────────────────────────────────────────────────────────
function IntentRow({ intent }: { intent: Intent }) {
  const color = protocolColor(intent.protocol)
  const isGreen = intent.stage === 'confirmed'
  const isDry = intent.stage === 'dry_run'
  const isSkip = intent.stage === 'skipped' || intent.stage === 'failed' || intent.stage === 'reverted'

  const amtNum = parseFloat(intent.amount) || 0
  const decimals = amtNum > 1e15 ? 18 : amtNum > 1e5 ? 6 : 0
  const amtDisplay = decimals > 0 ? (amtNum / Math.pow(10, decimals)).toFixed(2) : amtNum.toFixed(2)

  return (
    <div className={`border-l-2 pl-3 py-2 rounded-r transition-all ${isGreen ? 'bg-green-950/30' : isDry ? 'bg-yellow-950/20' : isSkip ? 'opacity-50' : 'bg-gray-950'}`}
      style={{ borderLeftColor: color }}>
      <div className="flex items-center justify-between gap-2 flex-wrap">
        <div className="flex items-center gap-2 min-w-0">
          <ProtoPill proto={intent.protocol} />
          <Chain id={intent.src_chain} />
          <span className="text-gray-500 text-xs">→</span>
          <Chain id={intent.dst_chain} />
          <span className="font-mono text-xs text-gray-400">{amtDisplay}</span>
        </div>
        <div className="flex items-center gap-2">
          {intent.profit_usd !== undefined && intent.profit_usd !== null && (
            <span className={`font-mono text-xs ${intent.profit_usd > 0 ? 'text-green-400' : 'text-gray-500'}`}>
              ${intent.profit_usd.toFixed(2)}
            </span>
          )}
          {intent.tx_hash && (
            <span className="font-mono text-[10px] text-blue-400">{intent.tx_hash.slice(0, 10)}…</span>
          )}
          <StageBadge stage={intent.stage} />
        </div>
      </div>
      <LifecycleBar stage={intent.stage} />
    </div>
  )
}

// ── Protocol panel ───────────────────────────────────────────────────────────
const KNOWN_PROTOCOLS = ['across_v3', 'debridge_dln', 'mayan_swift', 'lifi', 'orbiter_finance']
const PROTO_NAMES: Record<string, string> = {
  across_v3: 'Across V3', debridge_dln: 'deBridge DLN', mayan_swift: 'Mayan Swift',
  lifi: 'LiFi', orbiter_finance: 'Orbiter'
}

function ProtocolPanel({ protocols }: { protocols: Record<string, ProtoStats> }) {
  return (
    <div className="bg-gray-950 border border-gray-800 rounded-xl p-4">
      <div className="text-xs font-bold text-gray-400 uppercase tracking-widest mb-3">5 Protocols</div>
      <div className="space-y-3">
        {KNOWN_PROTOCOLS.map(key => {
          const stat = Object.entries(protocols).find(([k]) => k.includes(key.split('_')[0]))
          const data = stat?.[1]
          const color = protocolColor(key)
          const active = !!data && Date.now() - data.last_ms < 30_000
          return (
            <div key={key} className="flex items-center justify-between">
              <div className="flex items-center gap-2">
                <div className={`w-1.5 h-1.5 rounded-full ${active ? 'animate-pulse' : 'opacity-30'}`}
                  style={{ background: color }} />
                <span className="text-xs font-medium" style={{ color: active ? color : '#4A5568' }}>
                  {PROTO_NAMES[key]}
                </span>
              </div>
              {data ? (
                <div className="flex items-center gap-2 text-[10px] font-mono">
                  <span className="text-gray-500">{data.seen} seen</span>
                  {data.dry_run > 0 && <span className="text-yellow-500">{data.dry_run} dry</span>}
                  {data.confirmed > 0 && <span className="text-green-400">{data.confirmed} ✓</span>}
                  {data.skipped > 0 && <span className="text-gray-600">{data.skipped} skip</span>}
                </div>
              ) : (
                <span className="text-[10px] text-gray-700 font-mono">waiting…</span>
              )}
            </div>
          )
        })}
      </div>
    </div>
  )
}

// ── Live event ticker ─────────────────────────────────────────────────────────
function EventTicker({ events }: { events: LiveEvent[] }) {
  return (
    <div className="bg-gray-950 border border-gray-800 rounded-xl p-4 h-full flex flex-col">
      <div className="text-xs font-bold text-gray-400 uppercase tracking-widest mb-3">Event Stream</div>
      <div className="flex-1 overflow-y-auto space-y-1.5 pr-1" style={{ maxHeight: 340 }}>
        {events.length === 0 && <div className="text-gray-700 text-xs text-center py-8">No events yet…</div>}
        {events.map((e, i) => (
          <div key={i} className="flex items-start gap-2 text-[11px]">
            <span className="text-gray-700 font-mono shrink-0">{new Date(e.ts).toLocaleTimeString()}</span>
            <div className="flex items-center gap-1.5 flex-wrap">
              {e.protocol && <span className="font-bold" style={{ color: protocolColor(e.protocol) }}>{protocolLabel(e.protocol)}</span>}
              <span className={`
                ${e.type === 'solved' ? 'text-green-400' : e.type === 'failed' ? 'text-red-400' : e.stage === 'dry_run' ? 'text-yellow-400' : 'text-gray-400'}
              `}>{e.detail}</span>
              {e.tx_hash && (
                <span className="font-mono text-blue-400">{e.tx_hash.slice(0, 8)}…</span>
              )}
            </div>
          </div>
        ))}
      </div>
    </div>
  )
}

// ── Stats row ─────────────────────────────────────────────────────────────────
function StatCard({ label, value, color = 'text-white' }: { label: string; value: string | number; color?: string }) {
  return (
    <div className="bg-gray-950 border border-gray-800 rounded-xl px-4 py-3 flex flex-col gap-1">
      <span className="text-[10px] text-gray-500 uppercase tracking-widest">{label}</span>
      <span className={`text-xl font-mono font-bold ${color}`}>{value}</span>
    </div>
  )
}

// ── Main dashboard ────────────────────────────────────────────────────────────
export default function Dashboard() {
  const { intents, stats, protocols, events, logs, connected } = useSolverEvents()

  const dryRuns = intents.filter(i => i.stage === 'dry_run').length
  const confirmed = intents.filter(i => i.stage === 'confirmed').length
  const skipped = intents.filter(i => i.stage === 'skipped').length

  return (
    <div className="min-h-screen bg-[#050507] text-white font-sans" style={{ fontFamily: 'Inter, sans-serif' }}>
      {/* Header */}
      <header className="border-b border-gray-800/80 px-6 py-3 flex items-center justify-between">
        <div className="flex items-center gap-3">
          <div className="w-2 h-2 rounded-full bg-orange-500" />
          <span className="font-bold text-sm tracking-wide">TAIFOON SOLVER</span>
          <span className="text-gray-700 text-xs">hackathon · 5 protocols</span>
        </div>
        <div className="flex items-center gap-4">
          <div className="flex items-center gap-1.5">
            <div className={`w-2 h-2 rounded-full ${connected ? 'bg-green-400 animate-pulse' : 'bg-gray-700'}`} />
            <span className="text-xs text-gray-400">{connected ? 'LIVE' : 'CONNECTING'}</span>
          </div>
          <span className="text-xs font-mono text-green-400 font-bold">
            P&L: ${(stats?.net_profit_today_usd ?? 0).toFixed(4)}
          </span>
        </div>
      </header>

      {/* Stats row */}
      <div className="grid grid-cols-2 sm:grid-cols-4 lg:grid-cols-8 gap-2 px-4 py-3">
        <StatCard label="Intents" value={stats?.total_intents ?? intents.length} />
        <StatCard label="Dry Runs" value={dryRuns} color="text-yellow-400" />
        <StatCard label="Confirmed" value={confirmed} color="text-green-400" />
        <StatCard label="Skipped" value={skipped} color="text-gray-500" />
        <StatCard label="Fills" value={stats?.executed_fills ?? 0} color="text-green-400" />
        <StatCard label="Failed" value={stats?.failed_fills ?? 0} color="text-red-400" />
        <StatCard label="Success%" value={`${((stats?.success_rate ?? 0) * 100).toFixed(0)}%`} color="text-blue-400" />
        <StatCard label="Latency" value={`${stats?.latency_ms ?? 0}ms`} color="text-purple-400" />
      </div>

      {/* Main content */}
      <div className="grid grid-cols-1 lg:grid-cols-3 gap-3 px-4 pb-6">
        {/* Intent stream — 2 cols */}
        <div className="lg:col-span-2 space-y-3">
          {/* Lambda lifecycle legend */}
          <div className="bg-gray-950 border border-gray-800 rounded-xl p-3">
            <div className="text-[10px] text-gray-500 uppercase tracking-widest mb-2">Lambda Lifecycle</div>
            <div className="flex flex-wrap gap-2">
              {(['detected', 'profitability_check', 'calldata_build', 'estimate_gate', 'broadcast', 'confirmed', 'dry_run', 'skipped', 'failed'] as LambdaStage[]).map(s => (
                <StageBadge key={s} stage={s} />
              ))}
            </div>
          </div>

          {/* Intents */}
          <div className="bg-gray-950 border border-gray-800 rounded-xl">
            <div className="flex items-center justify-between px-4 py-3 border-b border-gray-800">
              <span className="text-xs font-bold text-gray-400 uppercase tracking-widest">Intent Stream</span>
              <span className="text-xs text-gray-600 font-mono">{intents.length} tracked</span>
            </div>
            <div className="divide-y divide-gray-900 max-h-[580px] overflow-y-auto px-3 py-1">
              {intents.length === 0 && (
                <div className="text-gray-700 text-xs text-center py-12">Waiting for intents…</div>
              )}
              {intents.map(intent => (
                <div key={intent.id} className="py-1.5">
                  <IntentRow intent={intent} />
                </div>
              ))}
            </div>
          </div>
        </div>

        {/* Right column */}
        <div className="space-y-3">
          <ProtocolPanel protocols={protocols} />
          <EventTicker events={events} />

          {/* Lambda stage breakdown */}
          <div className="bg-gray-950 border border-gray-800 rounded-xl p-4">
            <div className="text-xs font-bold text-gray-400 uppercase tracking-widest mb-3">Stage Breakdown</div>
            {(['detected', 'calldata_build', 'dry_run', 'confirmed', 'skipped', 'failed', 'reverted'] as LambdaStage[]).map(stage => {
              const count = intents.filter(i => i.stage === stage).length
              const pct = intents.length > 0 ? (count / intents.length) * 100 : 0
              return (
                <div key={stage} className="mb-2">
                  <div className="flex justify-between text-[11px] mb-0.5">
                    <span style={{ color: stageColor(stage) }}>{stageLabel(stage)}</span>
                    <span className="font-mono text-gray-500">{count}</span>
                  </div>
                  <div className="h-1 bg-gray-900 rounded-full overflow-hidden">
                    <div className="h-full rounded-full transition-all duration-500"
                      style={{ width: `${pct}%`, background: stageColor(stage) }} />
                  </div>
                </div>
              )
            })}
          </div>

          {/* Fill path status for each protocol */}
          <div className="bg-gray-950 border border-gray-800 rounded-xl p-4">
            <div className="text-xs font-bold text-gray-400 uppercase tracking-widest mb-3">Fill Paths</div>
            <div className="space-y-2 text-[11px]">
              {[
                { key: 'across', label: 'Across V3', method: 'fillV3Relay', contract: '0x5c7B…' },
                { key: 'debridge', label: 'deBridge DLN', method: 'fulfillOrder', contract: '0xeF4f…' },
                { key: 'mayan', label: 'Mayan Swift', method: 'fulfillOrder', contract: 'Swift' },
                { key: 'lifi', label: 'LiFi', method: 'meta-router', contract: 'LiFi Diamond' },
              ].map(row => {
                const active = Object.keys(protocols).some(k => k.includes(row.key))
                return (
                  <div key={row.key} className="flex items-center gap-2">
                    <div className={`w-1.5 h-1.5 rounded-full shrink-0 ${active ? 'bg-green-400' : 'bg-gray-700'}`} />
                    <span style={{ color: protocolColor(row.key) }} className="font-bold w-24 shrink-0">{row.label}</span>
                    <span className="text-gray-600 font-mono truncate">{row.method}()</span>
                    <span className="text-gray-700 font-mono text-[10px] shrink-0">{row.contract}</span>
                  </div>
                )
              })}
            </div>
          </div>

          {/* Live Solver Logs */}
          <div className="bg-gray-950 border border-gray-800 rounded-xl p-4">
            <div className="text-xs font-bold text-gray-400 uppercase tracking-widest mb-3">Solver Logs</div>
            <div className="overflow-y-auto space-y-0.5 font-mono text-[10px] leading-relaxed"
              style={{ maxHeight: 280 }}>
              {logs.length === 0 && <div className="text-gray-700 text-center py-6">No logs yet…</div>}
              {logs.map((line, i) => {
                const isErr = line.includes('ERROR') || line.includes('❌')
                const isWarn = line.includes('WARN') || line.includes('⚠')
                const isOk = line.includes('✅') || line.includes('🎉') || line.includes('confirmed')
                const isDry = line.includes('DRY_RUN') || line.includes('🧪')
                return (
                  <div key={i} className={
                    isErr ? 'text-red-400' :
                    isWarn ? 'text-yellow-400' :
                    isOk ? 'text-green-400' :
                    isDry ? 'text-yellow-300' :
                    'text-gray-500'
                  }>{line}</div>
                )
              })}
            </div>
          </div>
        </div>
      </div>
    </div>
  )
}
