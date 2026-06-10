'use client'

/**
 * arc-matrix — fetches coverage scoreboard from arc-api.
 *
 * /v1/matrix/coverage returns 31 protocols with chains_supported,
 * matrix_cells, has_fill_adapter flags. Powers the heatmap on /arc/matrix.
 */

import { useEffect, useState } from 'react'

import { resolveArcApiUrl } from './sse'

export type CoverageTier = 'executable' | 'family_wired' | 'catalogued'

export type MatrixProtocolRow = {
  slug:               string
  name:               string
  family:             string
  fee_model:          string
  chains_supported:   number
  chains:             number[]
  solana:             boolean
  matrix_cells:       number
  has_fill_adapter:   boolean
  fallback_behavior:  string
  coverage_tier:      CoverageTier
  routed_via?:        string | null
}

export type MatrixCoverage = {
  summary: {
    total_protocols:        number
    wired_protocols:        number
    unwired_protocols:      number
    total_cells:            number
    wired_cells:            number
    unwired_cells:          number
    wired_coverage_pct:     number
    executable_protocols?:  number
    family_wired_protocols?:number
    catalogued_protocols?:  number
    executable_cells?:      number
    family_wired_cells?:    number
    catalogued_cells?:      number
    routable_coverage_pct?: number
  }
  wired_adapters: string[]
  protocols:      MatrixProtocolRow[]
}

export type MatrixState = {
  loading:   boolean
  data:      MatrixCoverage | null
  error:     string | null
  fetchedAt: number | null
}

export function useMatrixCoverage(): MatrixState {
  const [state, setState] = useState<MatrixState>({
    loading: true, data: null, error: null, fetchedAt: null,
  })

  useEffect(() => {
    let cancelled = false
    let timer: ReturnType<typeof setTimeout> | null = null

    const tick = async () => {
      const url = `${resolveArcApiUrl()}/v1/matrix/coverage`
      try {
        const ctrl = new AbortController()
        const t = setTimeout(() => ctrl.abort(), 15_000)
        const resp = await fetch(url, { signal: ctrl.signal })
        clearTimeout(t)
        if (cancelled) return
        if (!resp.ok) {
          setState((s) => ({ ...s, loading: false, error: `HTTP ${resp.status}` }))
          return
        }
        const parsed = (await resp.json()) as MatrixCoverage
        setState({ loading: false, data: parsed, error: null, fetchedAt: Date.now() })
      } catch (e) {
        if (!cancelled) {
          const err = e as { message?: string }
          setState((s) => ({ ...s, loading: false, error: err.message ?? 'fetch failed' }))
        }
      } finally {
        // Re-poll every 30s — the wire-list rarely changes but the
        // request is cheap and surfaces deploys quickly.
        if (!cancelled) timer = setTimeout(tick, 30_000)
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
