'use client'

/**
 * Claims tab — visibility into the deBridge claim lifecycle.
 *
 * Reads from solver-api:
 *   GET  /api/solver/claims                    → all rows + pending_count
 *   POST /api/solver/claims/:intent_id/retry   → fire claimUnlock
 *
 * Both endpoints are token-gated. The token is read from the
 * NEXT_PUBLIC_SOLVER_API_TOKEN env var (same convention used by other
 * privileged dashboard panels). Polls every 5s and refreshes the row after
 * a retry resolves.
 */

import { useCallback, useEffect, useState } from 'react'
import { Card, Badge, Button, Tag } from '@/components/ui'

interface ClaimRow {
  intent_id: string
  protocol: string
  src_chain: number
  dst_chain: number
  amount_usd: number
  wallet_state: string
  claim_status: 'pending' | 'claimed' | 'reverted'
  fill_tx_hash: string | null
  claim_tx_hash: string | null
  claim_fee_usd: number | null
  created_at: string
  age_minutes: number
  error: string | null
}

interface ClaimsResponse {
  claims: ClaimRow[]
  pending_count: number
  as_of: string
}

const POLL_MS = 5_000
const SOLVER_API_BASE =
  (typeof process !== 'undefined' && process.env?.NEXT_PUBLIC_SOLVER_API_URL) || ''
const SOLVER_API_TOKEN =
  (typeof process !== 'undefined' && process.env?.NEXT_PUBLIC_SOLVER_API_TOKEN) || ''

const EXPLORERS: Record<number, string> = {
  1: 'https://etherscan.io/tx/',
  10: 'https://optimistic.etherscan.io/tx/',
  137: 'https://polygonscan.com/tx/',
  8453: 'https://basescan.org/tx/',
  42161: 'https://arbiscan.io/tx/',
  59144: 'https://lineascan.build/tx/',
  56: 'https://bscscan.com/tx/',
  43114: 'https://snowtrace.io/tx/',
}

function explorerUrl(chainId: number, tx: string | null): string | null {
  if (!tx) return null
  const base = EXPLORERS[chainId]
  return base ? `${base}${tx}` : null
}

function shortTx(tx: string | null): string {
  if (!tx) return '—'
  return tx.length > 12 ? `${tx.slice(0, 8)}…${tx.slice(-4)}` : tx
}

function ageLabel(mins: number): string {
  if (mins < 1) return '<1m'
  if (mins < 60) return `${mins}m`
  const hours = Math.floor(mins / 60)
  if (hours < 24) return `${hours}h ${mins % 60}m`
  const days = Math.floor(hours / 24)
  return `${days}d ${hours % 24}h`
}

function authHeaders(): HeadersInit {
  return SOLVER_API_TOKEN
    ? { Authorization: `Bearer ${SOLVER_API_TOKEN}` }
    : {}
}

// ── deBridge claim lifecycle guide (shown when no claims are in flight) ───────

const CLAIM_STEPS: Array<{ label: string; detail: string; contract?: string }> = [
  {
    label: 'ORDER DETECTED',
    detail: 'DlnSource.OrderCreated event arrives from the genome SSE feed. The solver decodes the order ID and token amounts from the on-chain log.',
    contract: 'DlnSource · 0xeF4fB24aD0916217251F553c0596F8Edc630EB66',
  },
  {
    label: 'PROFITABILITY CHECK',
    detail: 'profit-calc runs: take amount − give amount − estimated gas cost on the destination chain. Orders below MIN_PROFIT_USD are skipped immediately.',
  },
  {
    label: 'CALLDATA BUILD',
    detail: 'fulfillOrder() calldata is encoded for DlnDestination on the destination chain. Solver capital is reserved in the wallet DB under wallet_state = CALLDATA_BUILD.',
    contract: 'DlnDestination · 0xE7351Fd770A37282b91D153Ee690B63579D6dd7f',
  },
  {
    label: 'FILL TX BROADCAST',
    detail: 'fulfillOrder() sent to the destination chain. Wallet transitions to BROADCAST. The solver waits for receipt confirmation (≤ 90s window).',
  },
  {
    label: 'FILL CONFIRMED',
    detail: 'Receipt arrives. Wallet moves to CONFIRMED. outcome_log records the fill tx hash and actual profit. This row appears in Live P&L.',
  },
  {
    label: 'CLAIM UNLOCK',
    detail: 'lambda_claim_debridge fires claimUnlock() on DlnSource (source chain). This releases the order\'s locked give tokens to the solver. Wallet moves to CLAIM_PENDING.',
    contract: 'DlnSource · 0xeF4fB24aD0916217251F553c0596F8Edc630EB66',
  },
  {
    label: 'CLAIMED',
    detail: 'claimUnlock() receipt confirmed. claim_tx_hash and claim_fee_usd written to the outcome row. Wallet state = CLAIMED. Capital is now fully recovered on the source chain.',
  },
]

function ClaimsLifecycleGuide() {
  return (
    <div className="py-6 space-y-5">
      <div>
        <div className="text-[10px] tracking-[0.24em] uppercase text-[var(--text-tertiary)] mb-1">
          No claims in flight
        </div>
        <p className="text-[11px] text-[var(--text-secondary)] font-mono leading-relaxed max-w-[600px]">
          deBridge DLN fills require a two-phase commit across chains. The solver fills the
          order on the destination chain, then calls <span className="text-[var(--brand-blue)]">claimUnlock()</span> on
          the source chain to recover capital. This panel tracks every open position in that second phase.
        </p>
      </div>

      <div>
        <div className="text-[10px] tracking-[0.24em] uppercase text-[var(--text-tertiary)] mb-3">
          Claim lifecycle
        </div>
        <div className="space-y-0">
          {CLAIM_STEPS.map((step, i) => {
            const isTerminal = i === CLAIM_STEPS.length - 1
            const isClaim = step.label.includes('CLAIM')
            const color = isTerminal
              ? 'var(--success)'
              : isClaim
                ? 'var(--brand-blue)'
                : 'var(--text-tertiary)'
            return (
              <div key={step.label} className="flex gap-3 pb-4">
                <div className="flex flex-col items-center shrink-0 pt-0.5">
                  <div
                    className="w-1.5 h-1.5 rounded-full shrink-0"
                    style={{ background: color }}
                  />
                  {i < CLAIM_STEPS.length - 1 && (
                    <div className="w-px flex-1 mt-1" style={{ background: 'var(--border-subtle)' }} />
                  )}
                </div>
                <div className="pb-1 min-w-0">
                  <div className="font-mono text-[10px] tracking-[0.16em] mb-0.5" style={{ color }}>
                    {step.label}
                  </div>
                  <div className="text-[11px] text-[var(--text-secondary)] leading-relaxed">
                    {step.detail}
                  </div>
                  {step.contract && (
                    <div className="font-mono text-[10px] text-[var(--text-tertiary)] mt-0.5 tracking-[0.04em]">
                      {step.contract}
                    </div>
                  )}
                </div>
              </div>
            )
          })}
        </div>
      </div>

      <div className="border-t border-[var(--border-subtle)] pt-4">
        <div className="text-[10px] tracking-[0.24em] uppercase text-[var(--text-tertiary)] mb-2">
          LiFi enrichment
        </div>
        <p className="text-[11px] text-[var(--text-secondary)] font-mono leading-relaxed max-w-[600px]">
          LiFi intents arrive via the genome feed with a Diamond proxy tx hash, not the
          underlying deposit tx. The solver calls the{' '}
          <span className="text-[var(--brand-blue)]">li.quest /v1/status</span> API to resolve
          the actual bridge slug (Across, deBridge, or Mayan) and the real deposit tx hash.
          Unresolved intents enter an 8-retry backoff window before being dropped.
          LiFi Diamond routes to DlnDestination for deBridge orders — the claim lifecycle above
          applies to those fills identically.
        </p>
      </div>
    </div>
  )
}

export default function ClaimsPanel() {
  const [data, setData] = useState<ClaimsResponse | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [retrying, setRetrying] = useState<string | null>(null)
  const [retryToast, setRetryToast] = useState<{
    intent: string
    msg: string
    ok: boolean
  } | null>(null)

  const refresh = useCallback(async () => {
    try {
      const r = await fetch(`${SOLVER_API_BASE}/api/solver/claims`, {
        headers: authHeaders(),
        cache: 'no-store',
      })
      if (r.status === 401 || r.status === 403) {
        setError('NEXT_PUBLIC_SOLVER_API_TOKEN missing or invalid')
        setLoading(false)
        return
      }
      if (r.status === 503) {
        const j = await r.json().catch(() => ({}))
        setError(j?.error ?? 'service unavailable')
        setLoading(false)
        return
      }
      if (!r.ok) {
        setError(`HTTP ${r.status}`)
        setLoading(false)
        return
      }
      const j: ClaimsResponse = await r.json()
      setData(j)
      setError(null)
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => {
    refresh()
    const id = setInterval(refresh, POLL_MS)
    return () => clearInterval(id)
  }, [refresh])

  const onRetry = useCallback(
    async (intent: ClaimRow) => {
      setRetrying(intent.intent_id)
      setRetryToast(null)
      try {
        const r = await fetch(
          `${SOLVER_API_BASE}/api/solver/claims/${encodeURIComponent(
            intent.intent_id,
          )}/retry`,
          {
            method: 'POST',
            headers: authHeaders(),
          },
        )
        const body = await r.json().catch(() => ({}))
        if (r.ok) {
          setRetryToast({
            intent: intent.intent_id,
            msg: `Claimed: ${shortTx(body.claim_tx_hash ?? null)} (fee $${(
              body.fee_usd ?? 0
            ).toFixed(4)})`,
            ok: true,
          })
        } else {
          setRetryToast({
            intent: intent.intent_id,
            msg: `${body.outcome ?? `HTTP ${r.status}`}: ${
              body.error ?? 'unknown'
            }`,
            ok: false,
          })
        }
      } catch (e) {
        setRetryToast({
          intent: intent.intent_id,
          msg: e instanceof Error ? e.message : String(e),
          ok: false,
        })
      } finally {
        setRetrying(null)
        // Always refresh — wallet state may have moved even on partial failure.
        refresh()
      }
    },
    [refresh],
  )

  const claims = data?.claims ?? []
  const pending = data?.pending_count ?? 0

  return (
    <Card padding="none">
      <div className="flex items-center justify-between px-4 py-3 border-b border-[var(--border-subtle)]">
        <div className="flex items-center gap-2">
          <Tag>Claims</Tag>
          {pending > 0 && (
            <span className="text-[10px] font-mono px-1.5 py-0.5 rounded bg-[#FFB80022] text-[var(--warning)] animate-pulse">
              {pending} PENDING
            </span>
          )}
          {claims.filter((c) => c.claim_status === 'claimed').length > 0 && (
            <span className="text-[10px] font-mono px-1.5 py-0.5 rounded bg-[#00FF8822] text-[var(--success)]">
              {claims.filter((c) => c.claim_status === 'claimed').length} CLAIMED
            </span>
          )}
          {claims.filter((c) => c.claim_status === 'reverted').length > 0 && (
            <span className="text-[10px] font-mono px-1.5 py-0.5 rounded bg-[#FF444422] text-[var(--danger)]">
              {claims.filter((c) => c.claim_status === 'reverted').length}{' '}
              REVERTED
            </span>
          )}
        </div>
        <span className="text-[10px] font-mono text-[var(--text-tertiary)]">
          {claims.length} rows · refresh 5s
        </span>
      </div>

      {error && (
        <div className="px-4 py-3 text-[11px] text-[var(--danger)] font-mono">
          {error}
        </div>
      )}
      {retryToast && (
        <div
          className={`px-4 py-2 text-[11px] font-mono border-b border-[var(--border-subtle)] ${
            retryToast.ok
              ? 'text-[var(--success)] bg-[#00FF8810]'
              : 'text-[var(--danger)] bg-[#FF444410]'
          }`}
        >
          [{retryToast.intent.slice(0, 16)}…] {retryToast.msg}
        </div>
      )}

      <div className="overflow-x-auto">
        <table className="w-full text-[11px]">
          <thead>
            <tr className="text-left text-[10px] font-mono tracking-[0.16em] uppercase text-[var(--text-tertiary)] border-b border-[var(--border-subtle)]">
              <th className="px-3 py-2">Intent</th>
              <th className="px-3 py-2">Fill TX</th>
              <th className="px-3 py-2">Status</th>
              <th className="px-3 py-2">Claim TX</th>
              <th className="px-3 py-2 text-right">Fee</th>
              <th className="px-3 py-2 text-right">Age</th>
              <th className="px-3 py-2 text-right">Action</th>
            </tr>
          </thead>
          <tbody>
            {loading && (
              <tr>
                <td
                  colSpan={7}
                  className="px-3 py-6 text-center text-[var(--text-tertiary)]"
                >
                  Loading…
                </td>
              </tr>
            )}
            {!loading && claims.length === 0 && !error && (
              <tr>
                <td colSpan={7} className="px-4 py-0">
                  <ClaimsLifecycleGuide />
                </td>
              </tr>
            )}
            {claims.map((c) => {
              const fillUrl = explorerUrl(c.dst_chain, c.fill_tx_hash)
              const claimUrl = explorerUrl(c.src_chain, c.claim_tx_hash)
              const tone =
                c.claim_status === 'claimed'
                  ? 'mint'
                  : c.claim_status === 'reverted'
                    ? 'danger'
                    : 'warning'
              const statusLabel =
                c.claim_status === 'pending'
                  ? `PENDING (${c.wallet_state})`
                  : c.claim_status.toUpperCase()
              return (
                <tr
                  key={c.intent_id}
                  className="border-b border-[var(--border-subtle)] hover:bg-[var(--bg-elevated)]"
                >
                  <td className="px-3 py-2 font-mono text-[var(--text-secondary)]">
                    <div className="flex flex-col">
                      <span title={c.intent_id}>
                        {c.intent_id.length > 24
                          ? `${c.intent_id.slice(0, 22)}…`
                          : c.intent_id}
                      </span>
                      <span className="text-[10px] text-[var(--text-tertiary)]">
                        {c.protocol} · ${c.amount_usd.toFixed(2)}
                      </span>
                    </div>
                  </td>
                  <td className="px-3 py-2 font-mono">
                    {fillUrl ? (
                      <a
                        href={fillUrl}
                        target="_blank"
                        rel="noopener noreferrer"
                        className="text-[var(--brand-cyan)] hover:underline"
                      >
                        {shortTx(c.fill_tx_hash)}
                      </a>
                    ) : (
                      <span className="text-[var(--text-tertiary)]">
                        {shortTx(c.fill_tx_hash)}
                      </span>
                    )}
                  </td>
                  <td className="px-3 py-2">
                    <Badge tone={tone} dot={c.claim_status === 'pending'}>
                      {statusLabel}
                    </Badge>
                  </td>
                  <td className="px-3 py-2 font-mono">
                    {claimUrl ? (
                      <a
                        href={claimUrl}
                        target="_blank"
                        rel="noopener noreferrer"
                        className="text-[var(--brand-cyan)] hover:underline"
                      >
                        {shortTx(c.claim_tx_hash)}
                      </a>
                    ) : (
                      <span className="text-[var(--text-tertiary)]">—</span>
                    )}
                  </td>
                  <td className="px-3 py-2 font-mono text-right text-[var(--text-secondary)]">
                    {c.claim_fee_usd !== null
                      ? `$${c.claim_fee_usd.toFixed(4)}`
                      : '—'}
                  </td>
                  <td className="px-3 py-2 font-mono text-right text-[var(--text-tertiary)]">
                    {ageLabel(c.age_minutes)}
                  </td>
                  <td className="px-3 py-2 text-right">
                    {c.claim_status === 'pending' ? (
                      <Button
                        variant="secondary"
                        size="sm"
                        onClick={() => onRetry(c)}
                        disabled={retrying === c.intent_id}
                      >
                        {retrying === c.intent_id ? '…' : 'RETRY'}
                      </Button>
                    ) : (
                      <span className="text-[10px] text-[var(--text-tertiary)] font-mono">
                        —
                      </span>
                    )}
                  </td>
                </tr>
              )
            })}
          </tbody>
        </table>
      </div>
    </Card>
  )
}
