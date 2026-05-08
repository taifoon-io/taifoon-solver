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
                <td
                  colSpan={7}
                  className="px-3 py-6 text-center text-[var(--success)]"
                >
                  ✅ No deBridge claims in flight
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
