'use client'

/**
 * Live P&L panel for the Frontier Hackathon demo.
 *
 * Reads from the new solver-api endpoints:
 *   - GET /api/solver/pnl      → aggregate realized USD + per-protocol breakdown
 *   - GET /api/solver/outcomes → most recent N fills with explorer URLs
 *
 * Polls every 3s. Self-contained — no extra deps beyond React. Renders
 * gracefully when the outcome DB is empty (shows "$0.00 / 0 fills").
 *
 * Mount on whichever operational page the loop agents pick — typically
 * dashboard/app/portal/[solverId]/page.tsx or the main `app/page.tsx`.
 */

import { useEffect, useMemo, useState } from 'react'

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
  predicted_profit_usd: number | null
  actual_profit_usd: number | null
  skip_reason: string | null
  error: string | null
}

const POLL_INTERVAL_MS = 3000
const SOLVER_API_BASE =
  (typeof process !== 'undefined' && process.env?.NEXT_PUBLIC_SOLVER_API_URL) || ''

const PROTOCOL_COLOR: Record<string, string> = {
  across_v3: '#FF7A59',
  across: '#FF7A59',
  debridge: '#7B61FF',
  dln: '#7B61FF',
  mayan_swift: '#00C2A8',
  mayan: '#00C2A8',
  lifi: '#FFD23F',
  lifi_v2: '#FFD23F',
}

function colorFor(proto: string): string {
  const key = proto.toLowerCase()
  for (const [k, v] of Object.entries(PROTOCOL_COLOR)) {
    if (key.includes(k)) return v
  }
  return '#888'
}

function fmtUsd(n: number): string {
  const abs = Math.abs(n)
  if (abs >= 1000) return `$${(n / 1000).toFixed(2)}k`
  return `$${n.toFixed(2)}`
}

function fmtAge(ts: string): string {
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

export default function LivePnL() {
  const [summary, setSummary] = useState<PnlSummary | null>(null)
  const [outcomes, setOutcomes] = useState<OutcomeRecord[]>([])
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    let cancelled = false
    const tick = async () => {
      try {
        const [pnlRes, outRes] = await Promise.all([
          fetch(`${SOLVER_API_BASE}/api/solver/pnl`, { cache: 'no-store' }),
          fetch(`${SOLVER_API_BASE}/api/solver/outcomes?limit=10`, { cache: 'no-store' }),
        ])
        if (!pnlRes.ok || !outRes.ok) throw new Error(`HTTP ${pnlRes.status}/${outRes.status}`)
        const pnl: PnlSummary = await pnlRes.json()
        const out: OutcomeRecord[] = await outRes.json()
        if (!cancelled) {
          setSummary(pnl)
          setOutcomes(out)
          setError(null)
        }
      } catch (e: unknown) {
        if (!cancelled) setError(e instanceof Error ? e.message : 'fetch failed')
      }
    }
    tick()
    const id = setInterval(tick, POLL_INTERVAL_MS)
    return () => {
      cancelled = true
      clearInterval(id)
    }
  }, [])

  const protocols = useMemo(() => {
    if (!summary) return [] as { name: string; fills: number; realized: number; pct: number; color: string }[]
    const total = Math.max(summary.realized_usd_total, 0.0001)
    return Object.entries(summary.by_protocol)
      .map(([name, p]) => ({
        name,
        fills: p.fills,
        realized: p.realized_usd,
        pct: (p.realized_usd / total) * 100,
        color: colorFor(name),
      }))
      .sort((a, b) => b.realized - a.realized)
  }, [summary])

  return (
    <section
      className="rounded-2xl border border-neutral-800 bg-neutral-950 p-5 shadow-lg"
      aria-label="Live P&L"
    >
      <header className="mb-4 flex items-baseline justify-between">
        <h2 className="text-lg font-semibold tracking-tight text-neutral-100">Live P&amp;L</h2>
        {error ? (
          <span className="text-xs text-rose-400">solver-api: {error}</span>
        ) : (
          <span className="text-xs text-neutral-500">refreshes every 3s</span>
        )}
      </header>

      {/* Top-line: realized USD + fill counts */}
      <div className="mb-5 grid grid-cols-3 gap-4">
        <Stat
          label="Realized"
          value={summary ? fmtUsd(summary.realized_usd_total) : '—'}
          accent={summary && summary.realized_usd_total > 0 ? 'positive' : 'neutral'}
        />
        <Stat label="Fills" value={summary ? String(summary.fills_total) : '—'} />
        <Stat label="Last 24h" value={summary ? String(summary.last_24h_count) : '—'} />
      </div>

      {/* Per-protocol stacked bar */}
      {protocols.length > 0 && (
        <div className="mb-5">
          <div className="mb-2 flex h-2 w-full overflow-hidden rounded-full bg-neutral-900">
            {protocols.map((p) => (
              <div
                key={p.name}
                style={{ width: `${p.pct}%`, backgroundColor: p.color }}
                title={`${p.name}: ${fmtUsd(p.realized)} (${p.fills} fills)`}
              />
            ))}
          </div>
          <ul className="flex flex-wrap gap-x-4 gap-y-1 text-xs">
            {protocols.map((p) => (
              <li key={p.name} className="flex items-center gap-2 text-neutral-400">
                <span className="inline-block h-2 w-2 rounded-full" style={{ backgroundColor: p.color }} />
                <span className="font-mono text-neutral-200">{p.name}</span>
                <span>{fmtUsd(p.realized)} · {p.fills}</span>
              </li>
            ))}
          </ul>
        </div>
      )}

      {/* Last 10 fills */}
      <div>
        <h3 className="mb-2 text-xs font-semibold uppercase tracking-wide text-neutral-500">
          Recent fills
        </h3>
        {outcomes.length === 0 ? (
          <p className="text-sm text-neutral-500">No fills yet — start broadcasting and they'll appear here.</p>
        ) : (
          <table className="w-full text-sm">
            <tbody>
              {outcomes.map((o) => (
                <tr key={o.intent_id} className="border-t border-neutral-900">
                  <td className="py-2 pr-2">
                    <span
                      className="inline-block h-2 w-2 rounded-full"
                      style={{ backgroundColor: colorFor(o.protocol) }}
                    />
                  </td>
                  <td className="py-2 pr-3 font-mono text-xs text-neutral-300">{o.protocol}</td>
                  <td className="py-2 pr-3 text-xs text-neutral-500">
                    {o.src_chain} → {o.dst_chain}
                  </td>
                  <td className="py-2 pr-3">
                    <span className={pillClass(o.decision)}>{o.decision}</span>
                  </td>
                  <td className="py-2 pr-3 text-right font-mono text-xs">
                    {o.actual_profit_usd != null ? (
                      <span className={o.actual_profit_usd >= 0 ? 'text-emerald-400' : 'text-rose-400'}>
                        {fmtUsd(o.actual_profit_usd)}
                      </span>
                    ) : (
                      <span className="text-neutral-600">—</span>
                    )}
                  </td>
                  <td className="py-2 pr-3 text-right">
                    {o.explorer_url ? (
                      <a
                        href={o.explorer_url}
                        target="_blank"
                        rel="noreferrer"
                        className="font-mono text-xs text-sky-400 hover:underline"
                      >
                        {o.tx_hash?.slice(0, 8)}…
                      </a>
                    ) : (
                      <span className="font-mono text-xs text-neutral-600">no tx</span>
                    )}
                  </td>
                  <td className="py-2 text-right text-xs text-neutral-500">{fmtAge(o.ts)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>
    </section>
  )
}

function Stat({
  label,
  value,
  accent = 'neutral',
}: {
  label: string
  value: string
  accent?: 'positive' | 'neutral'
}) {
  const colorClass = accent === 'positive' ? 'text-emerald-400' : 'text-neutral-100'
  return (
    <div className="rounded-lg bg-neutral-900/60 p-3">
      <div className="text-xs uppercase tracking-wide text-neutral-500">{label}</div>
      <div className={`mt-1 font-mono text-2xl font-semibold ${colorClass}`}>{value}</div>
    </div>
  )
}

function pillClass(decision: string): string {
  const base = 'inline-block rounded-full px-2 py-0.5 text-[10px] font-mono uppercase tracking-wide'
  if (decision === 'confirmed' || decision === 'execute' || decision === 'executed') return `${base} bg-emerald-900/40 text-emerald-300`
  if (decision === 'reverted') return `${base} bg-rose-900/40 text-rose-300`
  if (decision === 'dry_run') return `${base} bg-sky-900/40 text-sky-300`
  if (decision.startsWith('skip')) return `${base} bg-neutral-800 text-neutral-400`
  return `${base} bg-neutral-900 text-neutral-500`
}
