'use client'

import { useEffect, useState } from 'react'

interface ProtocolStats {
  protocol: string
  fill_count: number
  total_profit_usd: number
}

export default function ProtocolBreakdown() {
  const [protocols, setProtocols] = useState<ProtocolStats[]>([])
  const [loading, setLoading] = useState(true)

  useEffect(() => {
    const fetchProtocols = async () => {
      try {
        const res = await fetch('/solver-api/protocols')
        if (res.ok) {
          const data = await res.json()
          setProtocols(data.protocols || [])
        }
      } catch {
        // Silently fail
      } finally {
        setLoading(false)
      }
    }

    fetchProtocols()
    const interval = setInterval(fetchProtocols, 10000)

    return () => clearInterval(interval)
  }, [])

  return (
    <div className="bg-[#0A0A0A] border border-gray-800 rounded-lg p-6">
      <h2 className="text-lg font-bold mb-4">PROTOCOL BREAKDOWN</h2>
      {loading ? (
        <div className="text-gray-500 text-center py-4">Loading...</div>
      ) : protocols.length === 0 ? (
        <div className="text-gray-500 text-center py-4">No data yet</div>
      ) : (
        <div className="space-y-2 text-sm">
          {protocols.map((p) => (
            <div key={p.protocol} className="flex justify-between items-center">
              <span className="text-gray-300">{p.protocol}</span>
              <div className="flex items-center gap-3">
                <span className="font-mono text-white">{p.fill_count} fills</span>
                <span className="font-mono text-[#00FF88]">${p.total_profit_usd.toFixed(2)}</span>
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  )
}
