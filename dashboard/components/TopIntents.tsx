'use client'

import { useEffect, useState } from 'react'

interface TopIntent {
  id: string
  protocol: string
  profit_usd: number
  timestamp: string
}

export default function TopIntents() {
  const [topIntents, setTopIntents] = useState<TopIntent[]>([])
  const [loading, setLoading] = useState(true)

  useEffect(() => {
    const fetchTopIntents = async () => {
      try {
        const res = await fetch('/solver-api/intents')
        if (res.ok) {
          const data = await res.json()
          // Sort by profit and take top 10
          const intents = data.intents || []
          const sorted = [...intents]
            .filter((i: any) => i.profit_usd && i.profit_usd > 0)
            .sort((a: any, b: any) => (b.profit_usd || 0) - (a.profit_usd || 0))
            .slice(0, 10)
          setTopIntents(sorted)
        }
      } catch {
        // Silently fail
      } finally {
        setLoading(false)
      }
    }

    fetchTopIntents()
    const interval = setInterval(fetchTopIntents, 15000)

    return () => clearInterval(interval)
  }, [])

  return (
    <div className="bg-[#0A0A0A] border border-gray-800 rounded-lg p-6">
      <h2 className="text-lg font-bold mb-4">TOP INTENTS (24h)</h2>
      {loading ? (
        <div className="text-gray-500 text-center py-4">Loading...</div>
      ) : topIntents.length === 0 ? (
        <div className="text-gray-500 text-center py-4">No profitable intents yet</div>
      ) : (
        <div className="space-y-2 text-sm">
          {topIntents.map((intent, idx) => (
            <div key={intent.id} className="flex justify-between items-center">
              <div className="flex items-center gap-2">
                <span className="text-gray-500 font-mono">{idx + 1}.</span>
                <span className="text-gray-400">{intent.protocol}</span>
              </div>
              <span className="font-mono text-[#00FF88]">${intent.profit_usd.toFixed(2)}</span>
            </div>
          ))}
        </div>
      )}
    </div>
  )
}
