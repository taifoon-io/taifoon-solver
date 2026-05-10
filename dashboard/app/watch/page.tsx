'use client'

/**
 * /watch?address=0x…
 *
 * Public read-only portfolio view for any EVM address.
 * Connects an address (pasted or from localStorage) to the solver's live data:
 *   - Per-chain balances via GET /api/solver/portfolio?address=
 *   - The solver's live intent + fill feed (shared — one solver, many observers)
 *   - P&L summary and realized fills from the outcome log
 *
 * No wallet signing — on-chain balance reads are public.
 * No auth token required — portfolio endpoint accepts ?address= without Bearer.
 */

import { useEffect, useRef, useState, useCallback, Suspense } from 'react'
import { useRouter, useSearchParams } from 'next/navigation'
import Link from 'next/link'
import { NavBar, Footer, Card, CardHeader, Badge, StatTile, Tag, Button } from '@/components/ui'
import LivePnL from '@/components/LivePnL'
import WalletPin from '@/components/WalletPin'

const STORAGE_KEY = 'taifoon_pinned_wallet'
const POLL_MS = 30_000

function isValidEvm(addr: string): boolean {
  return /^0x[0-9a-fA-F]{40}$/.test(addr.trim())
}

function shortAddr(a: string): string {
  return `${a.slice(0, 6)}…${a.slice(-4)}`
}

function fmtNum(n: number | null | undefined, decimals = 2): string {
  if (n === null || n === undefined) return '—'
  if (n === 0) return '0'
  if (Math.abs(n) < 0.0001) return n.toExponential(2)
  return n.toFixed(decimals)
}

function fmtUsd(n: number): string {
  if (n >= 1000) return `$${(n / 1000).toFixed(2)}k`
  return `$${n.toFixed(2)}`
}

function fmtAge(ts: string | null): string {
  if (!ts) return '—'
  const ms = Date.now() - new Date(ts).getTime()
  if (ms < 0) return 'now'
  const s = Math.floor(ms / 1000)
  if (s < 60) return `${s}s ago`
  const m = Math.floor(s / 60)
  if (m < 60) return `${m}m ago`
  const h = Math.floor(m / 60)
  if (h < 24) return `${h}h ago`
  return `${Math.floor(h / 24)}d ago`
}

const ETH_PRICE_USD = 3500
const SOL_PRICE_USD = 150
const SOLANA_CHAIN_ID = 1_399_811_149

interface ChainInventory {
  chain_id: number
  chain_name: string
  native_eth?: number | null
  native_sol?: number | null
  usdc?: number | null
  usdt?: number | null
  weth?: number | null
}

interface PortfolioResponse {
  solver_address: string
  solana_address?: string | null
  chains: ChainInventory[]
  fills: { confirmed: number; reverted: number; active: number; total_volume_usd: number; realized_profit_usd: number }
  as_of: string
  solana_sol_balance?: number | null
  solana_gas_status?: string | null
}

interface PnlSummary {
  realized_usd_total: number
  fills_total: number
  last_24h_count: number
  by_protocol: Record<string, { fills: number; realized_usd: number; avg_profit_usd: number }>
}

interface OutcomeRecord {
  ts: string
  intent_id: string
  protocol: string
  src_chain: number
  dst_chain: number
  decision: string
  tx_hash: string | null
  explorer_url: string | null
  actual_profit_usd: number | null
  skip_reason: string | null
}

function rowUsd(r: ChainInventory): number {
  const stable = (r.usdc ?? 0) + (r.usdt ?? 0)
  const eth = (r.native_eth ?? 0) * ETH_PRICE_USD
  const weth = (r.weth ?? 0) * ETH_PRICE_USD
  const sol = (r.native_sol ?? 0) * SOL_PRICE_USD
  return stable + eth + weth + sol
}

const CHAIN_EXPLORER: Record<number, string> = {
  1: 'https://etherscan.io/address/',
  10: 'https://optimistic.etherscan.io/address/',
  137: 'https://polygonscan.com/address/',
  8453: 'https://basescan.org/address/',
  42161: 'https://arbiscan.io/address/',
  59144: 'https://lineascan.build/address/',
  324: 'https://explorer.zksync.io/address/',
  56: 'https://bscscan.com/address/',
}

function explorerUrl(chainId: number, addr: string): string | null {
  const base = CHAIN_EXPLORER[chainId]
  return base ? `${base}${addr}` : null
}

const PROTOCOL_COLOR: Record<string, string> = {
  across_v3: '#3DA5FF',
  debridge_dln: '#FF8A4C',
  mayan_swift: '#9945FF',
  lifi: '#F1C40F',
}
function protoColor(p: string): string {
  for (const [k, v] of Object.entries(PROTOCOL_COLOR)) {
    if (p.toLowerCase().includes(k.split('_')[0])) return v
  }
  return '#94B0C4'
}

function ChainRow({ row }: { row: ChainInventory }) {
  const isSolana = row.chain_id === SOLANA_CHAIN_ID
  const usd = rowUsd(row)
  return (
    <tr className="border-b border-[var(--border-subtle)] hover:bg-[var(--bg-raised)] transition-colors">
      <td className="py-2 pr-3">
        <div className="flex items-center gap-2">
          <span className="w-1.5 h-1.5 rounded-full bg-[var(--brand-blue)]" aria-hidden />
          <span className="text-[var(--text-primary)] text-[12px] font-mono">{row.chain_name}</span>
        </div>
      </td>
      <td className="text-right py-2 pr-3 text-[var(--text-secondary)] font-mono text-[12px]">
        {fmtNum(row.usdc, 2)}
      </td>
      <td className="text-right py-2 pr-3 text-[var(--text-secondary)] font-mono text-[12px]">
        {fmtNum(row.usdt, 2)}
      </td>
      <td className="text-right py-2 pr-3 text-[var(--text-secondary)] font-mono text-[12px]">
        {fmtNum(row.weth, 4)}
      </td>
      <td className="text-right py-2 pr-3 text-[var(--text-secondary)] font-mono text-[12px]">
        {isSolana ? `${fmtNum(row.native_sol, 4)} SOL` : `${fmtNum(row.native_eth, 5)} ETH`}
      </td>
      <td className="text-right py-2 font-mono text-[12px] text-[var(--solana-mint)]">
        {fmtUsd(usd)}
      </td>
    </tr>
  )
}

function FillRow({ rec }: { rec: OutcomeRecord }) {
  const isConfirmed = rec.decision === 'executed'
  const isSkip = rec.decision.startsWith('skip')
  const color = protoColor(rec.protocol)
  return (
    <div
      className="py-2 px-3 border-l-2 my-0.5 rounded-r"
      style={{
        borderLeftColor: color,
        background: isConfirmed ? '#14F19510' : isSkip ? 'transparent' : '#FF6B6B08',
        opacity: isSkip ? 0.55 : 1,
      }}
    >
      <div className="flex items-center justify-between gap-2 flex-wrap">
        <div className="flex items-center gap-2 min-w-0">
          <span
            className="text-[10px] font-bold px-1.5 py-0.5 rounded-full"
            style={{ color, border: `1px solid ${color}33`, background: `${color}11` }}
          >
            {rec.protocol.replace('_', ' ').toUpperCase()}
          </span>
          <span className="text-[10px] font-mono text-[var(--text-tertiary)]">
            c{rec.src_chain} → c{rec.dst_chain}
          </span>
        </div>
        <div className="flex items-center gap-2 shrink-0">
          {rec.actual_profit_usd !== null && (
            <span className={`font-mono text-[11px] ${rec.actual_profit_usd > 0 ? 'text-[var(--success)]' : 'text-[var(--text-tertiary)]'}`}>
              {rec.actual_profit_usd > 0 ? '+' : ''}{rec.actual_profit_usd.toFixed(4)}
            </span>
          )}
          {rec.tx_hash && rec.explorer_url ? (
            <a
              href={rec.explorer_url}
              target="_blank"
              rel="noreferrer"
              className="font-mono text-[10px] text-[var(--brand-blue)] hover:underline"
            >
              {rec.tx_hash.slice(0, 10)}…
            </a>
          ) : null}
          <span
            className="text-[10px] px-1.5 py-0.5 rounded font-mono"
            style={{
              color: isConfirmed ? 'var(--success)' : isSkip ? 'var(--text-tertiary)' : 'var(--danger)',
              background: isConfirmed ? '#14F19514' : isSkip ? 'transparent' : '#FF6B6B14',
            }}
          >
            {rec.decision}
          </span>
          <span className="font-mono text-[10px] text-[var(--text-tertiary)]">
            {fmtAge(rec.ts)}
          </span>
        </div>
      </div>
    </div>
  )
}

// ── Main watch page (inner — uses hooks that need Suspense) ─────────────────

function WatchInner() {
  const searchParams = useSearchParams()
  const router = useRouter()

  const [address, setAddress] = useState<string | null>(null)
  const [portfolio, setPortfolio] = useState<PortfolioResponse | null>(null)
  const [pnl, setPnl] = useState<PnlSummary | null>(null)
  const [fills, setFills] = useState<OutcomeRecord[]>([])
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [lastAt, setLastAt] = useState<string | null>(null)

  // Resolve address: URL param first, then localStorage
  useEffect(() => {
    const fromUrl = searchParams.get('address')
    if (fromUrl && isValidEvm(fromUrl)) {
      setAddress(fromUrl)
      return
    }
    try {
      const stored = localStorage.getItem(STORAGE_KEY)
      if (stored && isValidEvm(stored)) {
        setAddress(stored)
        router.replace(`/watch?address=${encodeURIComponent(stored)}`)
      }
    } catch {}
  }, [searchParams, router])

  const refresh = useCallback(async (addr: string) => {
    try {
      const [pRes, plRes, oRes] = await Promise.all([
        fetch(`/api/solver/portfolio?address=${encodeURIComponent(addr)}`, { cache: 'no-store' }),
        fetch('/api/solver/pnl', { cache: 'no-store' }),
        fetch('/api/solver/outcomes?limit=50', { cache: 'no-store' }),
      ])
      if (pRes.ok) {
        const d: PortfolioResponse = await pRes.json()
        setPortfolio(d)
        setLastAt(d.as_of)
        setError(null)
      } else {
        setError(`Portfolio fetch failed (HTTP ${pRes.status})`)
      }
      if (plRes.ok) setPnl(await plRes.json())
      if (oRes.ok) {
        const recs = await oRes.json() as OutcomeRecord[]
        setFills(recs)
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => {
    if (!address) return
    setLoading(true)
    refresh(address)
    const id = setInterval(() => refresh(address), POLL_MS)
    return () => clearInterval(id)
  }, [address, refresh])

  const totalUsd = portfolio?.chains.reduce((a, r) => a + rowUsd(r), 0) ?? 0
  const executedFills = fills.filter((f) => f.decision === 'executed')

  if (!address) {
    return (
      <>
        <NavBar />
        <main className="flex-1 max-w-[860px] mx-auto px-6 py-20 flex flex-col items-center gap-6 text-center">
          <Tag>Portfolio Watch</Tag>
          <h1 className="tf-display tf-gradient-silver text-[2rem]">Pin your wallet</h1>
          <p className="text-[var(--text-secondary)] text-sm max-w-[480px] leading-relaxed">
            Enter any EVM address to see its cross-chain inventory alongside the solver&apos;s live fill feed.
            No signing required — balances are public on-chain reads.
          </p>
          <WalletPin onPin={(addr) => setAddress(addr)} />
          <p className="text-[var(--text-tertiary)] text-[11px] font-mono">
            Or go directly to{' '}
            <code className="text-[var(--brand-blue)]">/watch?address=0x…</code>
          </p>
        </main>
        <Footer />
      </>
    )
  }

  return (
    <>
      <NavBar />
      <main className="flex-1">
        {/* Header */}
        <div className="border-b border-[var(--border-subtle)] bg-[var(--bg-elevated)]">
          <div className="max-w-[1400px] mx-auto px-6 py-5 flex items-center justify-between flex-wrap gap-3">
            <div className="flex items-center gap-4 flex-wrap">
              <Link
                href="/portal"
                className="font-mono text-[11px] tracking-[0.2em] uppercase text-[var(--text-tertiary)] hover:text-[var(--brand-blue)] transition-colors"
              >
                ← PORTAL
              </Link>
              <span className="text-[var(--border-default)]">/</span>
              <div className="flex items-center gap-2">
                <span className="w-1.5 h-1.5 rounded-full bg-[var(--solana-mint)] animate-pulse" aria-hidden />
                <span className="font-mono text-[13px] text-[var(--text-primary)] tracking-[0.06em]">
                  {shortAddr(address)}
                </span>
                <Badge tone="mint">PINNED</Badge>
              </div>
              {lastAt && (
                <span className="font-mono text-[10px] text-[var(--text-tertiary)]">
                  refreshed {fmtAge(lastAt)}
                </span>
              )}
            </div>
            <div className="flex items-center gap-3">
              <Button
                href={`/watch?address=${encodeURIComponent(address)}`}
                variant="ghost"
                size="sm"
              >
                SHARE ↗
              </Button>
              <WalletPin onPin={(addr) => setAddress(addr)} />
            </div>
          </div>
        </div>

        {/* Summary stats */}
        <div className="max-w-[1400px] mx-auto grid grid-cols-2 sm:grid-cols-4 gap-x-8 gap-y-4 px-6 py-6 border-b border-[var(--border-subtle)]">
          <StatTile label="TOTAL USD" value={fmtUsd(totalUsd)} tone="mint" />
          <StatTile label="CHAINS" value={String(portfolio?.chains.length ?? '—')} tone="blue" />
          <StatTile label="FILLS (ALL)" value={String(pnl?.fills_total ?? '—')} />
          <StatTile label="REALIZED P&L" value={pnl ? `$${pnl.realized_usd_total.toFixed(2)}` : '—'} tone={pnl && pnl.realized_usd_total >= 0 ? 'mint' : 'danger'} />
        </div>

        <div className="max-w-[1400px] mx-auto grid grid-cols-1 lg:grid-cols-5 gap-4 px-6 py-6">
          {/* Left: portfolio */}
          <div className="lg:col-span-3 space-y-4">
            {/* Portfolio card */}
            <Card padding="none">
              <div className="flex items-center justify-between px-4 py-3 border-b border-[var(--border-subtle)]">
                <div className="flex items-center gap-2">
                  <Tag>Portfolio</Tag>
                  <span className="font-mono text-[10px] text-[var(--text-tertiary)]">
                    {address}
                  </span>
                </div>
                {/* Per-chain explorer links */}
                <div className="flex items-center gap-1.5">
                  {[1, 8453, 42161, 137, 10].map((cid) => {
                    const url = explorerUrl(cid, address)
                    const labels: Record<number, string> = { 1: 'ETH', 8453: 'BASE', 42161: 'ARB', 137: 'MATIC', 10: 'OP' }
                    return url ? (
                      <a
                        key={cid}
                        href={url}
                        target="_blank"
                        rel="noreferrer"
                        className="font-mono text-[9px] text-[var(--text-tertiary)] hover:text-[var(--brand-blue)] border border-[var(--border-subtle)] px-1.5 py-0.5 rounded transition-colors"
                        title={`View on chain ${cid}`}
                      >
                        {labels[cid]}↗
                      </a>
                    ) : null
                  })}
                </div>
              </div>

              {loading && !portfolio && (
                <div className="text-center py-10 text-[var(--text-tertiary)] text-sm font-mono">
                  Fetching balances…
                </div>
              )}
              {error && (
                <div className="text-center py-6 text-[var(--danger)] text-xs font-mono px-4">
                  {error}
                </div>
              )}
              {portfolio && (
                <div className="p-4">
                  <div className="overflow-x-auto">
                    <table className="w-full">
                      <thead>
                        <tr className="text-[10px] tracking-[0.16em] uppercase text-[var(--text-tertiary)] border-b border-[var(--border-subtle)]">
                          <th className="text-left py-2 pr-3">Chain</th>
                          <th className="text-right py-2 pr-3">USDC</th>
                          <th className="text-right py-2 pr-3">USDT</th>
                          <th className="text-right py-2 pr-3">WETH</th>
                          <th className="text-right py-2 pr-3">Gas</th>
                          <th className="text-right py-2">USD</th>
                        </tr>
                      </thead>
                      <tbody>
                        {portfolio.chains.map((row) => (
                          <ChainRow key={row.chain_id} row={row} />
                        ))}
                      </tbody>
                    </table>
                  </div>
                  <div className="mt-3 flex justify-end">
                    <span className="font-mono text-[11px] text-[var(--text-tertiary)]">
                      Total est.{' '}
                      <span className="text-[var(--solana-mint)] text-[13px]">{fmtUsd(totalUsd)}</span>
                    </span>
                  </div>
                </div>
              )}
            </Card>

            {/* Solver live P&L (shared — from outcome log) */}
            <LivePnL />
          </div>

          {/* Right: live fills */}
          <div className="lg:col-span-2 space-y-4">
            <Card padding="none">
              <div className="flex items-center justify-between px-4 py-3 border-b border-[var(--border-subtle)]">
                <Tag>Live Fills</Tag>
                <div className="flex items-center gap-2">
                  <span className="w-1.5 h-1.5 rounded-full bg-[var(--solana-mint)] animate-pulse" />
                  <span className="font-mono text-[10px] text-[var(--text-tertiary)]">
                    {executedFills.length} confirmed
                  </span>
                </div>
              </div>
              <div className="overflow-y-auto px-2 py-1" style={{ maxHeight: 520 }}>
                {fills.length === 0 && (
                  <div className="text-center py-10 text-[var(--text-tertiary)] text-xs font-mono">
                    No fills yet…
                  </div>
                )}
                {fills.map((f) => (
                  <FillRow key={f.intent_id + f.ts} rec={f} />
                ))}
              </div>
            </Card>

            {/* P&L by protocol */}
            {pnl && Object.keys(pnl.by_protocol).length > 0 && (
              <Card padding="md">
                <CardHeader title="By Protocol" />
                <div className="space-y-2">
                  {Object.entries(pnl.by_protocol).map(([proto, stat]) => (
                    <div key={proto} className="flex items-center justify-between text-[11px]">
                      <span
                        className="font-mono"
                        style={{ color: protoColor(proto) }}
                      >
                        {proto.replace('_', ' ').toUpperCase()}
                      </span>
                      <div className="flex gap-4 font-mono text-[var(--text-tertiary)]">
                        <span>{stat.fills} fills</span>
                        <span className={stat.realized_usd > 0 ? 'text-[var(--success)]' : ''}>
                          ${stat.realized_usd.toFixed(2)}
                        </span>
                      </div>
                    </div>
                  ))}
                </div>
              </Card>
            )}

            {/* Onboard CTA */}
            <Card padding="md" accent>
              <div className="text-[11px] font-mono text-[var(--text-tertiary)] mb-2">
                Want to earn from fills on this fleet?
              </div>
              <Button href="/onboard" variant="primary" size="sm">
                SPIN UP A SOLVER →
              </Button>
            </Card>
          </div>
        </div>
      </main>
      <Footer />
    </>
  )
}

export default function WatchPage() {
  return (
    <Suspense fallback={
      <div className="flex-1 flex items-center justify-center">
        <div className="font-mono text-[var(--text-tertiary)] text-sm animate-pulse">Loading…</div>
      </div>
    }>
      <WatchInner />
    </Suspense>
  )
}
