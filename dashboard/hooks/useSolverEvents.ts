import { useEffect, useState } from 'react'

interface Intent {
  id: string
  protocol: string
  timestamp: string
  state: string
  profit_usd?: number
  tx_hash?: string
  src_chain: number
  dst_chain: number
  amount: string
  gas_cost_usd?: number
  protocol_fee_usd?: number
}

interface Stats {
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

export function useSolverEvents() {
  const [intents, setIntents] = useState<Intent[]>([])
  const [stats, setStats] = useState<Stats | null>(null)
  const [connected, setConnected] = useState(false)

  useEffect(() => {
    // Load initial intents
    fetch('/solver-api/intents')
      .then(res => res.json())
      .then(data => setIntents(data.intents || []))
      .catch(err => console.error('Failed to load initial intents:', err))

    const eventSource = new EventSource('/solver-api/stream')

    eventSource.onopen = () => setConnected(true)
    eventSource.onerror = () => setConnected(false)

    eventSource.addEventListener('intent_detected', (e) => {
      const intent = JSON.parse(e.data)
      setIntents(prev => [intent, ...prev].slice(0, 50))
    })

    eventSource.addEventListener('intent_attempted', (e) => {
      const result = JSON.parse(e.data)
      setIntents(prev => prev.map(i =>
        i.id === result.id
          ? { ...i, state: result.decision, profit_usd: result.profit_usd, gas_cost_usd: result.gas_cost_usd, protocol_fee_usd: result.protocol_fee_usd }
          : i
      ))
    })

    eventSource.addEventListener('intent_solved', (e) => {
      const result = JSON.parse(e.data)
      setIntents(prev => prev.map(i =>
        i.id === result.id
          ? { ...i, state: 'solved', tx_hash: result.tx_hash }
          : i
      ))
    })

    const statsInterval = setInterval(async () => {
      try {
        const res = await fetch('/solver-api/stats')
        if (res.ok) setStats(await res.json())
      } catch {}
    }, 5000)

    return () => {
      eventSource.close()
      clearInterval(statsInterval)
    }
  }, [])

  return { intents, stats, connected }
}
