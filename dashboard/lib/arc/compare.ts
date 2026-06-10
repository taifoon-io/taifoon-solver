'use client'

/**
 * arc-compare — fetches multi-protocol pricing from /v1/compare.
 *
 * Surfaces every executable protocol for (src, dst, asset, amount) with
 * full per-row breakdown: gas / protocol fee / donut surcharge + 70/20/10
 * split / amount_out / total cost. Refetches on a configurable interval
 * (default 30s) so the dashboard tracks live drift.
 */

import { useEffect, useState } from 'react'

import { resolveArcApiUrl } from './sse'

export type CompareDonutSplit = {
  contributor_usd: number
  treasury_usd:    number
  solver_usd:      number
}

export type CompareRow = {
  protocol_slug:       string
  protocol_name:       string
  family:              string
  fee_model:           string
  fee_bps:             number
  coverage_tier:       string
  executable:          boolean
  estimated_gas:       number
  gas_cost_wei:        string
  gas_cost_usd:        number
  protocol_fee_usd:    number
  donut_surcharge_usd: number
  donut_split:         CompareDonutSplit
  amount_out_usd:      number
  total_cost_usd:      number
  delta_vs_best_usd:   number
  source:              string
  best?:               boolean
}

export type CompareResponse = {
  src_chain_id: number
  dst_chain_id: number
  dst_kind:     string
  input_amount: number
  asset:        string
  value_usd:    number
  native_price_usd_assumed: number
  fetched_at:   number
  best:         string | null
  protocol_count: number
  rows:         CompareRow[]
}

export type CompareRequest = {
  src_chain_id: number
  dst_chain_id: number
  dst_kind?:    'evm' | 'solana' | 'solana_devnet'
  src_token:    string
  dst_token:    string
  input_amount: number
  asset?:       string
}

export type CompareState = {
  loading:   boolean
  data:      CompareResponse | null
  error:     string | null
  fetchedAt: number | null
}

export function useArcCompare(req: CompareRequest, refreshMs: number = 30_000): CompareState {
  const [state, setState] = useState<CompareState>({
    loading: true, data: null, error: null, fetchedAt: null,
  })
  const key = JSON.stringify(req)

  useEffect(() => {
    let cancelled = false
    let timer: ReturnType<typeof setTimeout> | null = null

    const tick = async () => {
      const url = `${resolveArcApiUrl()}/v1/compare`
      try {
        const ctrl = new AbortController()
        const t = setTimeout(() => ctrl.abort(), 10_000)
        const resp = await fetch(url, {
          method: 'POST',
          headers: { 'content-type': 'application/json' },
          body:    JSON.stringify(req),
          signal:  ctrl.signal,
        })
        clearTimeout(t)
        if (cancelled) return
        if (!resp.ok) {
          setState((s) => ({ ...s, loading: false, error: `HTTP ${resp.status}` }))
          return
        }
        const parsed = (await resp.json()) as CompareResponse
        setState({ loading: false, data: parsed, error: null, fetchedAt: Date.now() })
      } catch (e) {
        if (!cancelled) {
          const err = e as { message?: string }
          setState((s) => ({ ...s, loading: false, error: err.message ?? 'fetch failed' }))
        }
      } finally {
        if (!cancelled) timer = setTimeout(tick, refreshMs)
      }
    }
    void tick()
    return () => {
      cancelled = true
      if (timer) clearTimeout(timer)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, refreshMs])

  return state
}

/**
 * The 7 curated "Live" cells from the Phase 4 plan — known-good real-world
 * routes we always want priced in the dashboard, regardless of what the
 * user is currently looking at. USDC addresses are the canonical mainnet
 * deployments; Solana endpoints use the placeholder "SOL" sentinel that
 * arc-api accepts when dst_kind="solana".
 */
export const CURATED_LIVE_CELLS: { label: string; req: CompareRequest }[] = [
  {
    label: 'Across · Op → Base · USDC',
    req: {
      src_chain_id: 10, dst_chain_id: 8453,
      src_token: '0x0b2C639c533813f4Aa9D7837CAf62653d097Ff85',
      dst_token: '0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913',
      input_amount: 100_000_000, asset: 'USDC',
    },
  },
  {
    label: 'Across · Arb → Base · USDC',
    req: {
      src_chain_id: 42161, dst_chain_id: 8453,
      src_token: '0xaf88d065e77c8cC2239327C5EDb3A432268e5831',
      dst_token: '0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913',
      input_amount: 100_000_000, asset: 'USDC',
    },
  },
  {
    label: 'Across · Eth → Arb · USDC',
    req: {
      src_chain_id: 1, dst_chain_id: 42161,
      src_token: '0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48',
      dst_token: '0xaf88d065e77c8cC2239327C5EDb3A432268e5831',
      input_amount: 100_000_000, asset: 'USDC',
    },
  },
  {
    label: 'Mayan · Arb → Solana · USDC',
    req: {
      src_chain_id: 42161, dst_chain_id: 0, dst_kind: 'solana',
      src_token: '0xaf88d065e77c8cC2239327C5EDb3A432268e5831',
      dst_token: 'EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v',
      input_amount: 100_000_000, asset: 'USDC',
    },
  },
  {
    label: 'Mayan · Solana → Base · USDC',
    req: {
      src_chain_id: 1, dst_chain_id: 8453,  // src_chain_id placeholder — Solana-source uses solana dst_kind flip later
      src_token: 'EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v',
      dst_token: '0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913',
      input_amount: 100_000_000, asset: 'USDC',
    },
  },
  {
    label: 'deBridge · Eth → Polygon · USDC',
    req: {
      src_chain_id: 1, dst_chain_id: 137,
      src_token: '0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48',
      dst_token: '0x3c499c542cEF5E3811e1192ce70d8cC03d5c3359',
      input_amount: 100_000_000, asset: 'USDC',
    },
  },
  {
    label: 'LiFi · Arb USDC ↔ WETH',
    req: {
      src_chain_id: 42161, dst_chain_id: 42161,
      src_token: '0xaf88d065e77c8cC2239327C5EDb3A432268e5831',
      dst_token: '0x82aF49447D8a07e3bd95BD0d56f35241523fBab1',
      input_amount: 100_000_000, asset: 'USDC',
    },
  },
]
