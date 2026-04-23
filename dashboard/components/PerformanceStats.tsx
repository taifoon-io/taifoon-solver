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

export default function PerformanceStats({ stats }: { stats: Stats | null }) {
  if (!stats) {
    return (
      <div className="bg-[#0A0A0A] border border-gray-800 rounded-lg p-6">
        <h2 className="text-lg font-bold mb-4">PERFORMANCE</h2>
        <div className="text-gray-500 text-center py-4">Loading...</div>
      </div>
    )
  }

  return (
    <div className="bg-[#0A0A0A] border border-gray-800 rounded-lg p-6">
      <h2 className="text-lg font-bold mb-4">PERFORMANCE</h2>
      <div className="space-y-3 text-sm">
        <div className="flex justify-between items-center">
          <span className="text-gray-400">Status</span>
          <span className={`font-bold ${stats.status === 'active' ? 'text-[#00FF88]' : 'text-gray-500'}`}>
            {stats.status.toUpperCase()}
          </span>
        </div>
        <div className="flex justify-between items-center">
          <span className="text-gray-400">Latency</span>
          <span className="font-mono text-white">{stats.latency_ms}ms</span>
        </div>
        <div className="flex justify-between items-center">
          <span className="text-gray-400">Success Rate</span>
          <span className="font-mono text-white">{(stats.success_rate * 100).toFixed(1)}%</span>
        </div>
        <div className="h-px bg-gray-800 my-2" />
        <div className="flex justify-between items-center">
          <span className="text-gray-400">Total Intents</span>
          <span className="font-mono text-white">{stats.total_intents}</span>
        </div>
        <div className="flex justify-between items-center">
          <span className="text-gray-400">Profitable</span>
          <span className="font-mono text-[#00FF88]">{stats.profitable_intents}</span>
        </div>
        <div className="flex justify-between items-center">
          <span className="text-gray-400">Skipped</span>
          <span className="font-mono text-gray-500">{stats.skipped_intents}</span>
        </div>
        <div className="h-px bg-gray-800 my-2" />
        <div className="flex justify-between items-center">
          <span className="text-gray-400">Executed Fills</span>
          <span className="font-mono text-[#00FF88]">{stats.executed_fills}</span>
        </div>
        <div className="flex justify-between items-center">
          <span className="text-gray-400">Failed Fills</span>
          <span className="font-mono text-[#FF3366]">{stats.failed_fills}</span>
        </div>
      </div>
    </div>
  )
}
