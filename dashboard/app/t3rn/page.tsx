'use client'

import { useCallback, useEffect, useState } from 'react'
import { NavBar, Footer, Card, Badge, StatTile, Button, Tag } from '@/components/ui'

// ── Types ─────────────────────────────────────────────────────────────────────

interface LwcChain {
  chain_id: number
  chain_key: string
  available_usd: number
  status: 'Healthy' | 'LowPool' | 'EmptyPool' | 'Halted' | 'NotDeployed'
}

interface SolverStatus {
  chains: LwcChain[]
  fills_last_hour: number
  hops_total: number
  dry_run: boolean
}

interface ChainInventory {
  chain_id: number
  chain_name: string
  native_eth?: number
  usdc?: number
  usdt?: number
  weth?: number
}

interface Portfolio {
  solver_address: string
  chains: ChainInventory[]
  lwc_chains: LwcChain[]
}

interface HopRecord {
  ts: number
  from_chain: number
  to_chain: number
  amount_usd: number
  status: string
  tx_src?: string
  tx_dst?: string
}

// ── Constants ─────────────────────────────────────────────────────────────────

const CHAIN_NAMES: Record<number, string> = {
  1: 'Ethereum',
  10: 'Optimism',
  56: 'BSC',
  130: 'Unichain',
  137: 'Polygon',
  8453: 'Base',
  42161: 'Arbitrum',
  59144: 'Linea',
}

const STATUS_CONFIG = {
  Healthy:     { tone: 'mint' as const,    label: 'HEALTHY',      dot: true,  pulse: true  },
  LowPool:     { tone: 'warning' as const, label: 'LOW POOL',     dot: true,  pulse: false },
  EmptyPool:   { tone: 'danger' as const,  label: 'EMPTY POOL',   dot: false, pulse: false },
  Halted:      { tone: 'danger' as const,  label: 'HALTED',       dot: true,  pulse: true  },
  NotDeployed: { tone: 'neutral' as const, label: 'NOT DEPLOYED', dot: false, pulse: false },
}

function fmt(n: number | undefined, dec = 2) {
  if (n === undefined || n === null) return '—'
  return n.toFixed(dec)
}

// ── Chain card ────────────────────────────────────────────────────────────────

function ChainCard({
  lwc,
  wallet,
}: {
  lwc: LwcChain | undefined
  wallet: ChainInventory | undefined
  chain_id: number
  chain_name: string
}) {
  const status = lwc?.status ?? 'NotDeployed'
  const cfg = STATUS_CONFIG[status] ?? STATUS_CONFIG.NotDeployed
  const available = lwc?.available_usd ?? 0
  const poolWidth = Math.min(100, (available / 10000) * 100)

  return (
    <Card padding="md" className="flex flex-col gap-3">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <span className="text-[13px] text-[var(--text-primary)] tracking-[0.06em]">
            {wallet?.chain_name ?? lwc?.chain_key ?? '—'}
          </span>
          <span className="font-mono text-[10px] text-[var(--text-tertiary)]">
            #{lwc?.chain_id ?? wallet?.chain_id}
          </span>
        </div>
        <Badge tone={cfg.tone} dot={cfg.dot} pulse={cfg.pulse}>
          {cfg.label}
        </Badge>
      </div>

      {/* Pool bar */}
      <div>
        <div className="flex justify-between text-[10px] font-mono mb-1">
          <span className="text-[var(--text-tertiary)] uppercase tracking-[0.16em]">LWC Pool</span>
          <span className="text-[var(--text-secondary)]">${fmt(available)}</span>
        </div>
        <div className="h-1.5 bg-[var(--bg-raised)] rounded-full overflow-hidden">
          <div
            className="h-full rounded-full transition-all duration-700"
            style={{
              width: `${poolWidth}%`,
              background:
                status === 'Healthy' ? 'var(--solana-mint)' :
                status === 'LowPool' ? 'var(--warning)' :
                'var(--danger)',
            }}
          />
        </div>
      </div>

      {/* Wallet balances */}
      <div className="grid grid-cols-3 gap-2 border-t border-[var(--border-subtle)] pt-2">
        <Micro label="ETH" value={wallet?.native_eth !== undefined ? fmt(wallet.native_eth, 4) : '—'} />
        <Micro label="USDC" value={wallet?.usdc !== undefined ? `$${fmt(wallet.usdc)}` : '—'} />
        <Micro label="WETH" value={wallet?.weth !== undefined ? fmt(wallet.weth, 4) : '—'} />
      </div>
    </Card>
  )
}

function Micro({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex flex-col gap-0.5">
      <span className="font-mono text-[9px] tracking-[0.2em] uppercase text-[var(--text-tertiary)]">{label}</span>
      <span className="font-mono text-[12px] tabular-nums text-[var(--text-secondary)]">{value}</span>
    </div>
  )
}

// ── Hop history ───────────────────────────────────────────────────────────────

function HopRow({ hop }: { hop: HopRecord }) {
  const from = CHAIN_NAMES[hop.from_chain] ?? `#${hop.from_chain}`
  const to = CHAIN_NAMES[hop.to_chain] ?? `#${hop.to_chain}`
  const ts = new Date(hop.ts * 1000).toLocaleTimeString()
  const statusColor =
    hop.status === 'done' ? 'var(--success)' :
    hop.status === 'failed' ? 'var(--danger)' :
    'var(--warning)'

  return (
    <div className="flex items-center justify-between text-[11px] py-1.5 border-b border-[var(--border-subtle)] last:border-0">
      <div className="flex items-center gap-2 font-mono">
        <span className="text-[var(--text-tertiary)]">{ts}</span>
        <span className="text-[var(--text-secondary)]">{from}</span>
        <span className="text-[var(--text-tertiary)]">→</span>
        <span className="text-[var(--text-secondary)]">{to}</span>
        <span className="text-[var(--brand-blue)]">${hop.amount_usd.toFixed(2)}</span>
      </div>
      <span className="font-mono text-[10px]" style={{ color: statusColor }}>
        {hop.status.toUpperCase()}
      </span>
    </div>
  )
}

// ── Main page ─────────────────────────────────────────────────────────────────

export default function T3rnPage() {
  const [status, setStatus]       = useState<SolverStatus | null>(null)
  const [portfolio, setPortfolio] = useState<Portfolio | null>(null)
  const [hops, setHops]           = useState<HopRecord[]>([])
  const [filling, setFilling]     = useState(false)
  const [fillResult, setFillResult] = useState<string | null>(null)
  const [loadErr, setLoadErr]     = useState<string | null>(null)

  const fetchAll = useCallback(async () => {
    try {
      const [statusRes, portfolioRes, hopsRes] = await Promise.allSettled([
        fetch('http://localhost:8092/t3rn/status').then(r => r.json()),
        fetch('/api/solver/portfolio').then(r => r.json()),
        fetch('http://localhost:8092/t3rn/hops').then(r => r.json()),
      ])

      if (statusRes.status === 'fulfilled') setStatus(statusRes.value)
      if (portfolioRes.status === 'fulfilled') setPortfolio(portfolioRes.value)
      if (hopsRes.status === 'fulfilled') setHops(hopsRes.value?.hops ?? [])
      if (statusRes.status === 'rejected' && portfolioRes.status === 'rejected') {
        setLoadErr('Could not reach solver API. Is the t3rn-solver running on port 8092?')
      } else {
        setLoadErr(null)
      }
    } catch {
      setLoadErr('Unexpected error fetching solver data.')
    }
  }, [])

  useEffect(() => {
    fetchAll()
    const id = setInterval(fetchAll, 5_000)
    return () => clearInterval(id)
  }, [fetchAll])

  const triggerTestFill = async () => {
    setFilling(true)
    setFillResult(null)
    try {
      const resp = await fetch('http://localhost:8092/t3rn/test-fill', { method: 'POST' })
      const data = await resp.json()
      if (data.error) {
        setFillResult(`Error: ${data.error}`)
      } else if (data.dry_run) {
        setFillResult(`DRY RUN — ${data.message}`)
      } else {
        setFillResult(`Submitted: ${data.tx_hash} — ${data.status}`)
      }
    } catch (e) {
      setFillResult(`Network error: ${e}`)
    } finally {
      setFilling(false)
    }
  }

  // Build union of all chains from LWC status + portfolio wallet
  const lwcByChain: Record<number, LwcChain> = {}
  ;(status?.chains ?? portfolio?.lwc_chains ?? []).forEach(c => { lwcByChain[c.chain_id] = c })

  const walletByChain: Record<number, ChainInventory> = {}
  ;(portfolio?.chains ?? []).forEach(c => { walletByChain[c.chain_id] = c })

  const allChainIds = Array.from(new Set([
    ...Object.keys(lwcByChain).map(Number),
    ...Object.keys(walletByChain).map(Number),
  ])).filter(id => id !== 999).sort((a, b) => a - b)

  const totalAvailableUsd = Object.values(lwcByChain).reduce((s, c) => s + (c.available_usd ?? 0), 0)
  const healthyChains = Object.values(lwcByChain).filter(c => c.status === 'Healthy').length
  const totalChains = Object.values(lwcByChain).length

  return (
    <>
      <NavBar />
      <main className="flex-1">
        {/* Page header */}
        <div className="border-b border-[var(--border-subtle)]">
          <div className="max-w-[1400px] mx-auto px-6 py-10 flex items-end justify-between flex-wrap gap-6">
            <div>
              <Tag>t3rn LWC</Tag>
              <h1 className="tf-display tf-gradient-silver mt-4 text-[clamp(1.6rem,3.5vw,2.6rem)]">
                Liquidity Well Portfolio
              </h1>
              <p className="mt-2 text-sm text-[var(--text-secondary)] max-w-[520px] leading-relaxed">
                Real-time view of the solver&apos;s capital across all 8 t3rn LWC V4 chains.
                Pool depth, wallet balances, and hop history in one place.
              </p>
            </div>
            <div className="flex items-center gap-3">
              <Button variant="secondary" size="sm" onClick={fetchAll}>
                REFRESH
              </Button>
              <Button
                variant="primary"
                size="sm"
                onClick={triggerTestFill}
                disabled={filling}
              >
                {filling ? 'SUBMITTING…' : 'TEST FILL BASE → OP'}
              </Button>
            </div>
          </div>

          {/* Stats bar */}
          <div className="max-w-[1400px] mx-auto px-6 pb-8 grid grid-cols-2 sm:grid-cols-5 gap-x-10 gap-y-4">
            <StatTile label="CHAINS" value={totalChains} />
            <StatTile label="HEALTHY" value={healthyChains} tone="mint" />
            <StatTile label="POOL TVL" value={`$${totalAvailableUsd.toFixed(0)}`} tone="blue" />
            <StatTile label="FILLS / SESSION" value={status?.fills_last_hour ?? '—'} tone="mint" />
            <StatTile label="HOPS TOTAL" value={status?.hops_total ?? '—'} />
          </div>
        </div>

        {/* Error / fill result banners */}
        <div className="max-w-[1400px] mx-auto px-6 pt-4 space-y-2">
          {loadErr && (
            <div className="text-[11px] font-mono text-[var(--warning)] bg-[#FFB80012] border border-[#FFB80030] rounded px-4 py-2">
              {loadErr}
            </div>
          )}
          {fillResult && (
            <div className={`text-[11px] font-mono px-4 py-2 rounded border ${
              fillResult.startsWith('Error')
                ? 'text-[var(--danger)] bg-[#FF444412] border-[#FF444430]'
                : 'text-[var(--success)] bg-[#00FF8812] border-[#00FF8830]'
            }`}>
              {fillResult}
            </div>
          )}
          {status?.dry_run && (
            <div className="text-[10px] font-mono text-[var(--warning)] bg-[#FFB80008] border border-[#FFB80020] rounded px-4 py-1.5">
              DRY_RUN=true — fills and hops are simulated only
            </div>
          )}
        </div>

        {/* Chain grid */}
        <section className="max-w-[1400px] mx-auto px-6 py-6">
          <div className="flex items-center gap-3 mb-4">
            <Tag>Chain States</Tag>
            <span className="font-mono text-[10px] text-[var(--text-tertiary)] tracking-[0.16em]">
              {allChainIds.length} chains · refreshes every 5s
            </span>
          </div>

          {allChainIds.length === 0 ? (
            <div className="text-[var(--text-tertiary)] text-sm text-center py-16">
              No chain data yet — waiting for t3rn solver…
            </div>
          ) : (
            <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-3">
              {allChainIds.map(chainId => (
                <ChainCard
                  key={chainId}
                  chain_id={chainId}
                  chain_name={CHAIN_NAMES[chainId] ?? `Chain ${chainId}`}
                  lwc={lwcByChain[chainId]}
                  wallet={walletByChain[chainId]}
                />
              ))}
            </div>
          )}
        </section>

        {/* Hop history */}
        <section className="max-w-[1400px] mx-auto px-6 pb-12">
          <Card padding="none">
            <div className="flex items-center justify-between px-4 py-3 border-b border-[var(--border-subtle)]">
              <Tag>Hop History</Tag>
              <span className="font-mono text-[10px] text-[var(--text-tertiary)] tracking-[0.12em]">
                {hops.length} hops
              </span>
            </div>
            <div className="px-4 py-2 max-h-[320px] overflow-y-auto">
              {hops.length === 0 ? (
                <div className="text-[var(--text-tertiary)] text-xs text-center py-8">
                  No hops yet — rebalancer runs every 5 minutes
                </div>
              ) : (
                hops.slice(0, 50).map((hop, i) => <HopRow key={i} hop={hop} />)
              )}
            </div>
          </Card>
        </section>

        {/* Test fill explainer */}
        <section className="max-w-[1400px] mx-auto px-6 pb-16">
          <Card padding="lg" accent>
            <div className="flex items-start justify-between flex-wrap gap-4">
              <div className="max-w-[640px]">
                <Tag>Base ↔ Optimism E2E Test</Tag>
                <h3 className="mt-3 text-lg font-light text-[var(--text-primary)]">
                  How the test fill works
                </h3>
                <ol className="mt-3 text-sm text-[var(--text-secondary)] space-y-1.5 leading-relaxed list-decimal list-inside">
                  <li>
                    <strong className="text-[var(--text-primary)]">Click &ldquo;TEST FILL BASE → OP&rdquo;</strong> — submits a real{' '}
                    <code className="font-mono text-[11px] bg-[var(--bg-raised)] px-1 py-0.5 rounded">order()</code> tx on Base LWC
                    with destination <code className="font-mono text-[11px] bg-[var(--bg-raised)] px-1 py-0.5 rounded">optm</code>
                    {' '}($1 USDC, max reward $1.10).
                  </li>
                  <li>
                    <strong className="text-[var(--text-primary)]">OrderMonitor</strong> polls Base for{' '}
                    <code className="font-mono text-[11px] bg-[var(--bg-raised)] px-1 py-0.5 rounded">OrderCreated</code> events every 2s
                    and broadcasts the decoded order.
                  </li>
                  <li>
                    <strong className="text-[var(--text-primary)]">SelfFill</strong> receives the broadcast, checks{' '}
                    <code className="font-mono text-[11px] bg-[var(--bg-raised)] px-1 py-0.5 rounded">canPerformInstantExecution</code>{' '}
                    on Optimism LWC, estimates gas, and fills the order on Optimism.
                  </li>
                  <li>
                    Fills counter increments and both chain statuses update here in real-time.
                  </li>
                </ol>
              </div>
              <div className="font-mono text-[11px] text-[var(--text-tertiary)] space-y-1 shrink-0">
                <div>Base LWC: <span className="text-[var(--brand-blue)]">0xb590…f45500</span></div>
                <div>Optimism LWC: <span className="text-[var(--brand-blue)]">0xa15f…98E6</span></div>
                <div>Destination tag: <span className="text-[var(--solana-mint)]">optm</span></div>
              </div>
            </div>
          </Card>
        </section>
      </main>
      <Footer />
    </>
  )
}
