'use client'

import { useEffect, useState } from 'react'

interface MoneyFlowData {
  protocol_fees_usd: number
  gas_costs_usd: number
  liquidity_costs_usd: number
  net_profit_usd: number
  roi_percentage: number
}

export default function MoneyFlow() {
  const [data, setData] = useState<MoneyFlowData | null>(null)
  const [loading, setLoading] = useState(true)

  useEffect(() => {
    const fetchMoneyFlow = async () => {
      try {
        const res = await fetch('http://localhost:8082/api/solver/money-flow')
        if (res.ok) {
          const flowData = await res.json()
          setData(flowData)
        }
      } catch {
        // Silently fail
      } finally {
        setLoading(false)
      }
    }

    fetchMoneyFlow()
    const interval = setInterval(fetchMoneyFlow, 10000)

    return () => clearInterval(interval)
  }, [])

  if (loading) {
    return (
      <div className="bg-[#0A0A0A] border border-gray-800 rounded-lg p-6">
        <h2 className="text-lg font-bold mb-4">MONEY FLOW</h2>
        <div className="text-gray-500 text-center py-4">Loading...</div>
      </div>
    )
  }

  if (!data) {
    return (
      <div className="bg-[#0A0A0A] border border-gray-800 rounded-lg p-6">
        <h2 className="text-lg font-bold mb-4">MONEY FLOW</h2>
        <div className="text-gray-500 text-center py-4">No data yet</div>
      </div>
    )
  }

  return (
    <div className="bg-[#0A0A0A] border border-gray-800 rounded-lg p-6">
      <h2 className="text-lg font-bold mb-4">MONEY FLOW</h2>
      <div className="space-y-3 text-sm">
        <div className="flex justify-between items-center">
          <span className="text-gray-400">Protocol Fees</span>
          <span className="font-mono text-[#00FF88]">+${data.protocol_fees_usd.toFixed(2)}</span>
        </div>
        <div className="flex justify-between items-center">
          <span className="text-gray-400">Gas Costs</span>
          <span className="font-mono text-[#FF3366]">-${data.gas_costs_usd.toFixed(2)}</span>
        </div>
        <div className="flex justify-between items-center">
          <span className="text-gray-400">Liquidity Costs</span>
          <span className="font-mono text-[#FF3366]">-${data.liquidity_costs_usd.toFixed(2)}</span>
        </div>
        <div className="h-px bg-gray-800 my-2" />
        <div className="flex justify-between items-center">
          <span className="text-white font-bold">Net Profit</span>
          <span className={`font-mono font-bold ${data.net_profit_usd >= 0 ? 'text-[#00FF88]' : 'text-[#FF3366]'}`}>
            {data.net_profit_usd >= 0 ? '+' : ''}${data.net_profit_usd.toFixed(2)}
          </span>
        </div>
        <div className="flex justify-between items-center">
          <span className="text-gray-400">ROI</span>
          <span className={`font-mono ${data.roi_percentage >= 0 ? 'text-[#00FF88]' : 'text-[#FF3366]'}`}>
            {data.roi_percentage.toFixed(1)}%
          </span>
        </div>
      </div>
    </div>
  )
}
