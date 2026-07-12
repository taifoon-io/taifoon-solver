'use client'

/**
 * arc-verdicts — fetches per-cell verdicts from /v1/matrix/cells/verdicts.
 *
 * Cached server-side per case_id; surfaced as a 2D grid on /arc/matrix.
 * Verdict shape: "A:Intent", "A:Catalogue", "L1:Header", "Approved", etc.
 * Approved → mint, A:* (arc-layer reject) → amber, L*:* (V5 reject) → blue,
 * empty → muted.
 */

import { useEffect, useState } from 'react'

import { resolveArcApiUrl } from './sse'

export type CellVerdict = {
  case_id:  string
  protocol: string
  src:      number
  dst:      number
  verdict:  string
  approved: boolean
  dry_run:  boolean
  ts:       number
}

export type VerdictsPayload = {
  summary: {
    total:       number
    approved:    number
    by_verdict:  Record<string, number>
    by_protocol: Record<string, number>
  }
  cells: CellVerdict[]
}

export type VerdictsState = {
  loading:   boolean
  data:      VerdictsPayload | null
  error:     string | null
  fetchedAt: number | null
}

export function useCellVerdicts(): VerdictsState {
  const [state, setState] = useState<VerdictsState>({
    loading: true, data: null, error: null, fetchedAt: null,
  })

  useEffect(() => {
    let cancelled = false
    let timer: ReturnType<typeof setTimeout> | null = null

    const tick = async () => {
      const url = `${resolveArcApiUrl()}/v1/matrix/cells/verdicts`
      try {
        const ctrl = new AbortController()
        const t = setTimeout(() => ctrl.abort(), 10_000)
        const resp = await fetch(url, { signal: ctrl.signal })
        clearTimeout(t)
        if (cancelled) return
        if (!resp.ok) {
          setState((s) => ({ ...s, loading: false, error: `HTTP ${resp.status}` }))
          return
        }
        const parsed = (await resp.json()) as VerdictsPayload
        setState({ loading: false, data: parsed, error: null, fetchedAt: Date.now() })
      } catch (e) {
        if (!cancelled) {
          const err = e as { message?: string }
          setState((s) => ({ ...s, loading: false, error: err.message ?? 'fetch failed' }))
        }
      } finally {
        if (!cancelled) timer = setTimeout(tick, 20_000)
      }
    }
    void tick()
    return () => {
      cancelled = true
      if (timer) clearTimeout(timer)
    }
  }, [])

  return state
}

/** Bucket a verdict string into a tone for cell colorization. */
export function verdictTone(v: string, approved: boolean): 'mint' | 'amber' | 'blue' | 'muted' {
  if (approved) return 'mint'
  if (!v || v === '?')  return 'muted'
  if (v.startsWith('A:')) return 'amber'   // arc-layer rejection
  if (/^L\d+/.test(v))    return 'blue'    // V5-layer rejection
  return 'muted'
}
