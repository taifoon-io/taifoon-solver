'use client'

/**
 * arc-sse — typed EventSource hook for /v1/devnet/dispatches/sse on
 * arc-api (taifoon-arc/crates/arc-api). Auto-reconnects with exponential
 * backoff, capped at 30s. Drops events past KEEP_LAST so memory stays bounded.
 *
 * Falls back to polling /v1/devnet/status every 10s so devnet rows render
 * even when SSE is blocked (CORS, proxy buffering, dev/HMR closing the
 * connection). The fallback only sets `latestTraces` — fills/reviews/donut
 * still require SSE.
 *
 * Resolution order for the arc-api base URL:
 *   1. `NEXT_PUBLIC_ARC_API_URL` (set per environment at build time)
 *   2. If page is on `solver.taifoon.dev` → `https://arc.taifoon.dev`
 *   3. Local dev fallback → `http://localhost:7088`
 *
 * Exposes a `connection` field with full diagnostics so the UI can show
 * the real failure mode instead of a silent empty state.
 */

import { useEffect, useRef, useState } from 'react'

export type ArcDispatchEvent =
  | { kind: 'quote'; ts: number; protocol_slug: string; src_chain_id: number; dst_chain_id: number; dst_kind: string; protocol_fee_usd: number; donut_surcharge: number; estimated_gas: number }
  | { kind: 'fill'; ts: number; intent_id: string; protocol_slug: string; src_chain_id: number; dst_chain_id: number; dst_kind: string; value_routed_usd: number; donut_fee_usd: number; status: string; fill_tx: string | null; via: string | null; sidecar_intent_id: string | null }
  | { kind: 'box_review'; ts: number; intent_id: string; protocol_slug: string; verdict: string; approved: boolean; dry_run: boolean }
  | { kind: 'devnet_trace'; ts: number; chain: string; chain_id: number; rpc_url: string; block: number; latency_ms: number; healthy: boolean }
  | { kind: 'donut_accrued'; ts: number; intent_id: string; protocol_slug: string; protocol_fee_usd: number; surcharge_usd: number; contributor_usd: number; treasury_usd: number; solver_usd: number; contributor_addr: string; token: string }

const KEEP_LAST = 250
const POLL_FALLBACK_MS = 10_000

export type ConnectionState = 'init' | 'connecting' | 'open' | 'error' | 'closed'

export type ArcStreamState = {
  connection: ConnectionState
  connected: boolean
  reconnects: number
  lastErrorAt: number | null
  endpoint: string
  events: ArcDispatchEvent[]
  /** Latest DevnetTrace per chain — drives the status cards. */
  latestTraces: Record<string, Extract<ArcDispatchEvent, { kind: 'devnet_trace' }>>
}

/** Resolve the arc-api base URL. Exported so the operator overlay shows it. */
export function resolveArcApiUrl(): string {
  const envUrl = typeof process !== 'undefined' ? process.env?.NEXT_PUBLIC_ARC_API_URL : undefined
  if (envUrl) return envUrl
  if (typeof window !== 'undefined') {
    const host = window.location.hostname
    if (host.endsWith('taifoon.dev') || host.endsWith('taifoon.io')) {
      return `${window.location.protocol}//arc.taifoon.dev`
    }
  }
  return 'http://localhost:7088'
}

export const ARC_API_URL_DEFAULT = resolveArcApiUrl()

export function useArcDispatchStream(): ArcStreamState {
  const [state, setState] = useState<ArcStreamState>({
    connection: 'init',
    connected: false,
    reconnects: 0,
    lastErrorAt: null,
    endpoint: '',
    events: [],
    latestTraces: {},
  })
  const esRef = useRef<EventSource | null>(null)

  useEffect(() => {
    const ARC_API_URL = resolveArcApiUrl()
    let cancelled = false
    let backoff = 1000
    let pollTimer: ReturnType<typeof setTimeout> | null = null

    function pushEvent(parsed: ArcDispatchEvent) {
      setState((s) => {
        const events = [parsed, ...s.events].slice(0, KEEP_LAST)
        const latestTraces = { ...s.latestTraces }
        if (parsed.kind === 'devnet_trace') {
          latestTraces[parsed.chain] = parsed
        }
        return { ...s, events, latestTraces }
      })
    }

    function pollDevnetStatus() {
      if (cancelled) return
      fetch(`${ARC_API_URL}/v1/devnet/status`)
        .then((r) => (r.ok ? r.json() : Promise.reject(new Error(String(r.status)))))
        .then((data: { devnets: Array<{ name: string; chain_id: number; rpc_url: string; block: number; latency_ms: number; healthy: boolean }> }) => {
          if (cancelled) return
          for (const d of data.devnets ?? []) {
            pushEvent({
              kind: 'devnet_trace',
              ts: Math.floor(Date.now() / 1000),
              chain: d.name,
              chain_id: d.chain_id,
              rpc_url: d.rpc_url,
              block: d.block,
              latency_ms: d.latency_ms,
              healthy: d.healthy,
            })
          }
        })
        .catch(() => {
          /* swallow — connection state already reflects SSE health */
        })
        .finally(() => {
          if (!cancelled) pollTimer = setTimeout(pollDevnetStatus, POLL_FALLBACK_MS)
        })
    }

    function connect() {
      if (cancelled) return
      const url = `${ARC_API_URL}/v1/devnet/dispatches/sse`
      setState((s) => ({ ...s, connection: 'connecting', endpoint: url }))

      let es: EventSource
      try {
        es = new EventSource(url)
      } catch (err) {
        setState((s) => ({ ...s, connection: 'error', lastErrorAt: Date.now() }))
        setTimeout(connect, backoff)
        backoff = Math.min(backoff * 2, 30_000)
        return
      }
      esRef.current = es

      // Generic message handler — defaults to no event name.
      es.onmessage = (ev) => {
        if (cancelled) return
        try {
          pushEvent(JSON.parse(ev.data) as ArcDispatchEvent)
        } catch {
          /* ignore malformed */
        }
      }

      es.addEventListener('hello', () => {
        if (cancelled) return
        backoff = 1000
        setState((s) => ({ ...s, connection: 'open', connected: true }))
      })

      const named = (ev: MessageEvent) => {
        if (cancelled) return
        try {
          pushEvent(JSON.parse(ev.data) as ArcDispatchEvent)
        } catch {
          /* ignore malformed */
        }
      }
      es.addEventListener('quote',         named)
      es.addEventListener('fill',          named)
      es.addEventListener('box_review',    named)
      es.addEventListener('devnet_trace',  named)
      es.addEventListener('donut_accrued', named)

      es.addEventListener('error', () => {
        if (cancelled) return
        setState((s) => ({
          ...s,
          connection: 'error',
          connected: false,
          reconnects: s.reconnects + 1,
          lastErrorAt: Date.now(),
        }))
        es.close()
        setTimeout(connect, backoff)
        backoff = Math.min(backoff * 2, 30_000)
      })
    }

    connect()
    // Always start the poll-fallback after a small delay so SSE has a chance.
    pollTimer = setTimeout(pollDevnetStatus, 2000)

    return () => {
      cancelled = true
      esRef.current?.close()
      if (pollTimer) clearTimeout(pollTimer)
    }
  }, [])

  return state
}
