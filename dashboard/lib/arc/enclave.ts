'use client'

/**
 * arc-enclave — operator-only health probe against the signer-deployer
 * loopback enclave. Returns null when unreachable (operator overlay
 * shows the "OFFLINE" state and the rest of the page keeps working).
 */

import { useEffect, useState } from 'react'

export const ARC_SIGNER_URL =
  (typeof process !== 'undefined' && process.env?.NEXT_PUBLIC_ARC_SIGNER_URL) ||
  'http://127.0.0.1:7575'

export type EnclaveHealth = {
  ok: boolean
  keys_loaded: string[]
  pending: number
  stress_mode: boolean
  devnet_chains: number[]
  bind_addr: string
}

export function useEnclaveHealth() {
  const [data, setData]     = useState<EnclaveHealth | null>(null)
  const [reach, setReach]   = useState(false)
  const [last, setLast]     = useState<number | null>(null)

  useEffect(() => {
    let cancelled = false
    let timer: ReturnType<typeof setTimeout> | null = null

    const tick = async () => {
      try {
        const ctrl = new AbortController()
        const t = setTimeout(() => ctrl.abort(), 2_000)
        const resp = await fetch(`${ARC_SIGNER_URL}/v1/health`, { signal: ctrl.signal })
        clearTimeout(t)
        if (cancelled) return
        if (!resp.ok) {
          setReach(false)
          return
        }
        const parsed = (await resp.json()) as EnclaveHealth
        setData(parsed)
        setReach(true)
        setLast(Date.now())
      } catch {
        if (!cancelled) setReach(false)
      } finally {
        if (!cancelled) timer = setTimeout(tick, 5_000)
      }
    }

    void tick()
    return () => {
      cancelled = true
      if (timer) clearTimeout(timer)
    }
  }, [])

  return { data, reachable: reach, lastChecked: last }
}
