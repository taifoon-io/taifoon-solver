'use client'

import { useState, useEffect } from 'react'

interface RazorGasPreset {
  chain_id: number
  chain_name: string
  ready: boolean
  symbol?: string
  gas_limit?: number
  gas_cost_wei?: string
  gas_cost_gwei?: number
  gas_cost_native?: number
  gas_cost_usd?: number
  max_fee_per_gas_gwei?: number
  max_priority_fee_gwei?: number
  price_usd?: number
  age_ms?: number
  reason?: string
}

interface RazorResponse {
  presets: RazorGasPreset[]
}

export default function RazorGasPresets() {
  const [data, setData] = useState<RazorResponse | null>(null)
  const [loading, setLoading] = useState(true)

  useEffect(() => {
    const fetchRazor = async () => {
      try {
        const response = await fetch('/solver-api/razor')
        if (response.ok) {
          const json = await response.json()
          setData(json)
          setLoading(false)
        }
      } catch (error) {
        console.error('Failed to fetch Razor data:', error)
        setLoading(false)
      }
    }

    // Initial fetch
    fetchRazor()

    // Refresh every 15 seconds
    const interval = setInterval(fetchRazor, 15000)

    return () => clearInterval(interval)
  }, [])

  if (loading) {
    return (
      <div className="bg-[#0A0A0A] border border-gray-800 rounded-lg p-6">
        <h2 className="text-lg font-bold mb-4">GAS PRESETS (RAZOR)</h2>
        <div className="text-gray-500 text-center py-4">Loading...</div>
      </div>
    )
  }

  if (!data || data.presets.length === 0) {
    return (
      <div className="bg-[#0A0A0A] border border-gray-800 rounded-lg p-6">
        <h2 className="text-lg font-bold mb-4">GAS PRESETS (RAZOR)</h2>
        <div className="text-gray-500 text-center py-4">No data available</div>
      </div>
    )
  }

  return (
    <div className="bg-[#0A0A0A] border border-gray-800 rounded-lg p-6">
      <h2 className="text-lg font-bold mb-4">GAS PRESETS (RAZOR)</h2>
      <div className="space-y-4">
        {data.presets.map((preset) => (
          <div
            key={preset.chain_id}
            className="border-b border-gray-800 pb-3 last:border-b-0 last:pb-0"
          >
            <div className="flex justify-between items-center mb-2">
              <span className="font-bold text-white">{preset.chain_name}</span>
              <span
                className={`text-xs px-2 py-1 rounded ${
                  preset.ready ? 'bg-[#00FF88] text-black' : 'bg-gray-700 text-gray-400'
                }`}
              >
                {preset.ready ? 'READY' : 'NOT READY'}
              </span>
            </div>
            {preset.ready ? (
              <div className="space-y-1.5 text-sm">
                <div className="flex justify-between items-center">
                  <span className="text-gray-400">Gas Cost ({preset.symbol})</span>
                  <span className="font-mono text-[#00FF88]">
                    {preset.gas_cost_native?.toFixed(6) ?? 'N/A'}
                  </span>
                </div>
                <div className="flex justify-between items-center">
                  <span className="text-gray-400">Gas Cost (USD)</span>
                  <span className="font-mono text-[#00FF88]">
                    ${preset.gas_cost_usd?.toFixed(4) ?? 'N/A'}
                  </span>
                </div>
                <div className="flex justify-between items-center">
                  <span className="text-gray-400">Max Fee</span>
                  <span className="font-mono text-white">
                    {preset.max_fee_per_gas_gwei?.toFixed(2) ?? 'N/A'} gwei
                  </span>
                </div>
                {preset.age_ms !== undefined && (
                  <div className="flex justify-between items-center">
                    <span className="text-gray-400">Age</span>
                    <span className="font-mono text-gray-500 text-xs">
                      {(preset.age_ms / 1000).toFixed(0)}s
                    </span>
                  </div>
                )}
              </div>
            ) : (
              <div className="text-sm text-gray-500">
                {preset.reason ?? 'Unavailable'}
              </div>
            )}
          </div>
        ))}
      </div>
    </div>
  )
}
