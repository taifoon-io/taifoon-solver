'use client'

import { useSolverEvents } from '@/hooks/useSolverEvents'
import IntentsStream from '@/components/IntentsStream'
import PerformanceStats from '@/components/PerformanceStats'
import ProtocolBreakdown from '@/components/ProtocolBreakdown'
import MoneyFlow from '@/components/MoneyFlow'
import TopIntents from '@/components/TopIntents'

export default function Dashboard() {
  const { intents, stats, connected } = useSolverEvents()

  return (
    <div className="min-h-screen bg-[#1A1A1A] text-white">
      <header className="border-b border-gray-800 px-6 py-4 flex justify-between items-center">
        <h1 className="text-2xl font-bold">Taifoon Solver</h1>
        <div className="flex items-center gap-4">
          <div className="flex items-center gap-2">
            <div className={`w-2 h-2 rounded-full ${connected ? 'bg-[#00FF88]' : 'bg-gray-600'} animate-pulse`} />
            <span className="text-sm text-gray-400">{connected ? 'LIVE' : 'OFFLINE'}</span>
          </div>
          <div className="text-xl font-bold text-[#00FF88]">
            Net: ${stats?.net_profit_today_usd.toFixed(2) ?? '0.00'}
          </div>
        </div>
      </header>

      <div className="grid grid-cols-1 lg:grid-cols-3 gap-6 p-6">
        {/* Left column - Intents Stream (takes 2/3 width on large screens) */}
        <div className="lg:col-span-2">
          <IntentsStream intents={intents} />
        </div>

        {/* Right column - Stats and Info (takes 1/3 width on large screens) */}
        <div className="space-y-6">
          <PerformanceStats stats={stats} />
          <ProtocolBreakdown />
          <MoneyFlow />
          <TopIntents />
        </div>
      </div>
    </div>
  )
}
