'use client'
import { useEffect, useState, useCallback, useRef } from 'react'

export type LambdaStage =
  | 'detected' | 'profitability_check' | 'proof_fetch' | 'calldata_build'
  | 'estimate_gate' | 'broadcast' | 'pending_confirmation'
  | 'confirmed' | 'reverted' | 'skipped' | 'failed' | 'dry_run'

export interface Intent {
  id: string
  protocol: string
  timestamp: string
  stage: LambdaStage
  profit_usd?: number
  protocol_fee_usd?: number
  gas_cost_usd?: number
  tx_hash?: string
  src_chain: number
  dst_chain: number
  amount: string
  token?: string
  skip_reason?: string
}

export interface ProtoStats {
  name: string
  seen: number
  dry_run: number
  confirmed: number
  skipped: number
  failed: number
  profit_usd: number
  last_ms: number
}

export interface SolverStats {
  status: string
  net_profit_today_usd: number
  latency_ms: number
  success_rate: number
  total_intents: number
  profitable_intents: number
  skipped_intents: number
  executed_fills: number
  failed_fills: number
}

export interface LiveEvent {
  ts: number
  type: 'detected' | 'attempted' | 'solved' | 'failed'
  protocol: string
  intent_id: string
  detail: string
  profit?: number
  tx_hash?: string
  stage?: LambdaStage
}

export function protocolColor(p: string): string {
  const l = p.toLowerCase()
  if (l.includes('across')) return '#00D9FF'
  if (l.includes('debridge') || l.includes('dln')) return '#FF6B2B'
  if (l.includes('mayan')) return '#9B59B6'
  if (l.includes('lifi') || l.includes('li.fi')) return '#F1C40F'
  if (l.includes('orbiter')) return '#95A5A6'
  return '#4A5568'
}

export function protocolLabel(p: string): string {
  const l = p.toLowerCase()
  if (l.includes('across')) return 'Across V3'
  if (l.includes('debridge') || l.includes('dln')) return 'deBridge DLN'
  if (l.includes('mayan')) return 'Mayan Swift'
  if (l.includes('lifi') || l.includes('li.fi')) return 'LiFi'
  if (l.includes('orbiter')) return 'Orbiter'
  return p
}

export function chainName(id: number): string {
  const m: Record<number, string> = {
    1: 'ETH', 10: 'OP', 137: 'MATIC', 8453: 'BASE', 42161: 'ARB',
    56: 'BSC', 59144: 'LINEA', 324: 'ZKSYNC', 1399811149: 'SOL',
  }
  return m[id] ?? `c${id}`
}

export function stageLabel(s: LambdaStage): string {
  const m: Record<LambdaStage, string> = {
    detected: 'DETECTED', profitability_check: 'PROFIT CHECK', proof_fetch: 'PROOF',
    calldata_build: 'CALLDATA', estimate_gate: 'ESTIMATE', broadcast: 'BROADCAST',
    pending_confirmation: 'PENDING', confirmed: 'CONFIRMED', reverted: 'REVERTED',
    skipped: 'SKIPPED', failed: 'FAILED', dry_run: 'DRY-RUN',
  }
  return m[s] ?? s.toUpperCase()
}

export function stageColor(s: LambdaStage): string {
  if (s === 'confirmed') return '#00FF88'
  if (s === 'dry_run') return '#F1C40F'
  if (s === 'reverted' || s === 'failed') return '#FF3366'
  if (s === 'skipped') return '#4A5568'
  if (s === 'broadcast' || s === 'pending_confirmation') return '#00D9FF'
  return '#94A3B8'
}

const API = typeof window !== 'undefined'
  ? (process.env.NEXT_PUBLIC_SOLVER_API_URL ?? 'http://localhost:8082')
  : 'http://localhost:8082'

export function useSolverEvents() {
  const [intents, setIntents] = useState<Intent[]>([])
  const [stats, setStats] = useState<SolverStats | null>(null)
  const [protocols, setProtocols] = useState<Record<string, ProtoStats>>({})
  const [events, setEvents] = useState<LiveEvent[]>([])
  const [connected, setConnected] = useState(false)

  const bump = useCallback((proto: string, delta: Partial<ProtoStats>) => {
    if (!proto) return
    const key = proto.toLowerCase()
    setProtocols(prev => {
      const cur = prev[key] ?? { name: proto, seen: 0, dry_run: 0, confirmed: 0, skipped: 0, failed: 0, profit_usd: 0, last_ms: 0 }
      return {
        ...prev,
        [key]: {
          ...cur,
          seen: cur.seen + (delta.seen ?? 0),
          dry_run: cur.dry_run + (delta.dry_run ?? 0),
          confirmed: cur.confirmed + (delta.confirmed ?? 0),
          skipped: cur.skipped + (delta.skipped ?? 0),
          failed: cur.failed + (delta.failed ?? 0),
          profit_usd: cur.profit_usd + (delta.profit_usd ?? 0),
          last_ms: Date.now(),
        }
      }
    })
  }, [])

  const pushEvent = useCallback((e: LiveEvent) => {
    setEvents(prev => [e, ...prev].slice(0, 300))
  }, [])

  useEffect(() => {
    fetch(`${API}/api/solver/intents`)
      .then(r => r.json())
      .then(d => {
        const recs = (d.intents ?? []) as any[]
        setIntents(recs.map(i => ({ ...i, stage: i.state ?? 'detected' })))
        recs.forEach(i => bump(i.protocol, { seen: 1 }))
      })
      .catch(() => {})

    const es = new EventSource(`${API}/api/solver/stream`)
    es.onopen = () => setConnected(true)
    es.onerror = () => setConnected(false)

    es.addEventListener('intent_detected', (e: MessageEvent) => {
      const d = JSON.parse(e.data)
      setIntents(prev => [{
        id: d.id, protocol: d.protocol, timestamp: d.timestamp,
        stage: 'detected', src_chain: d.src_chain, dst_chain: d.dst_chain,
        amount: d.amount, token: d.token,
      }, ...prev].slice(0, 150))
      bump(d.protocol, { seen: 1 })
      pushEvent({ ts: Date.now(), type: 'detected', protocol: d.protocol, intent_id: d.id,
        detail: `${chainName(d.src_chain)} → ${chainName(d.dst_chain)} ${d.amount.slice(0, 10)}` })
    })

    es.addEventListener('intent_attempted', (e: MessageEvent) => {
      const d = JSON.parse(e.data)
      const stage: LambdaStage = d.decision === 'execute' ? 'calldata_build' : d.decision === 'dry_run' ? 'dry_run' : 'skipped'
      setIntents(prev => prev.map(i =>
        i.id === d.id ? { ...i, stage, profit_usd: d.profit_usd, gas_cost_usd: d.gas_cost_usd, protocol_fee_usd: d.protocol_fee_usd } : i
      ))
      if (stage === 'skipped') bump(d.protocol ?? '', { skipped: 1 })
      if (stage === 'dry_run') bump(d.protocol ?? '', { dry_run: 1 })
      pushEvent({ ts: Date.now(), type: 'attempted', protocol: d.protocol ?? '', intent_id: d.id,
        detail: `${stage} profit=$${(d.profit_usd ?? 0).toFixed(2)}`, profit: d.profit_usd, stage })
    })

    es.addEventListener('intent_solved', (e: MessageEvent) => {
      const d = JSON.parse(e.data)
      setIntents(prev => prev.map(i => i.id === d.id ? { ...i, stage: 'confirmed', tx_hash: d.tx_hash } : i))
      pushEvent({ ts: Date.now(), type: 'solved', protocol: '', intent_id: d.id,
        detail: `tx ${d.tx_hash?.slice(0, 12)}… profit=$${(d.actual_profit_usd ?? 0).toFixed(4)}`,
        profit: d.actual_profit_usd, tx_hash: d.tx_hash, stage: 'confirmed' })
    })

    const si = setInterval(() => {
      fetch(`${API}/api/solver/stats`).then(r => r.ok ? r.json() : null).then(d => d && setStats(d)).catch(() => {})
    }, 3000)

    return () => { es.close(); clearInterval(si) }
  }, [bump, pushEvent])

  return { intents, stats, protocols, events, connected }
}
