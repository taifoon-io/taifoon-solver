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

export default function IntentsStream({ intents }: { intents: Intent[] }) {
  return (
    <div className="bg-[#0A0A0A] border border-gray-800 rounded-lg p-6">
      <h2 className="text-lg font-bold mb-4">INTENTS STREAM (Real-time)</h2>
      <div className="space-y-3 max-h-[700px] overflow-y-auto">
        {intents.length === 0 && (
          <div className="text-gray-500 text-center py-8">
            Waiting for intents...
          </div>
        )}
        {intents.map(intent => (
          <div
            key={intent.id}
            className="bg-[#1A1A1A] border border-gray-700 rounded p-4 transition-all hover:border-[#00D9FF]"
          >
            <div className="flex justify-between items-start mb-2">
              <div className="flex items-center gap-2">
                <span className="text-2xl">📥</span>
                <span className="font-mono text-sm text-[#00D9FF]">
                  {intent.protocol} #{intent.id.slice(-8)}
                </span>
              </div>
              <div className="text-right">
                {intent.state === 'solved' && (
                  <span className="text-[#00FF88] font-bold">✅ EXECUTED</span>
                )}
                {intent.state === 'execute' && intent.profit_usd && intent.profit_usd > 0 && (
                  <span className="text-[#00FF88] font-bold">💰 PROFITABLE</span>
                )}
                {intent.state === 'skip' && (
                  <span className="text-gray-500">⏭️ SKIP</span>
                )}
                {intent.state === 'detected' && (
                  <span className="text-gray-400">👀 DETECTED</span>
                )}
              </div>
            </div>
            <div className="text-sm text-gray-400 space-y-1">
              <div>
                Chain {intent.src_chain} → {intent.dst_chain} •{' '}
                {(parseInt(intent.amount || '0') / 1e6).toFixed(2)} USDC
              </div>
              {intent.profit_usd !== undefined && (
                <div className={`font-bold ${intent.profit_usd > 0 ? 'text-[#00FF88]' : 'text-gray-500'}`}>
                  Profit: ${intent.profit_usd.toFixed(2)}
                </div>
              )}
              {intent.gas_cost_usd !== undefined && intent.protocol_fee_usd !== undefined && (
                <div className="text-xs text-gray-500">
                  Gas: ${intent.gas_cost_usd.toFixed(2)} • Fee: ${intent.protocol_fee_usd.toFixed(2)}
                </div>
              )}
              {intent.tx_hash && (
                <div className="text-xs text-[#00D9FF] font-mono break-all">
                  {intent.tx_hash}
                </div>
              )}
            </div>
          </div>
        ))}
      </div>
    </div>
  )
}
