'use client'

/**
 * OperatorOverlay — collapsible bottom-fixed panel for the human operator.
 *
 * Shows reachability + key list + pending tx queue + stress mode of the
 * signer-deployer enclave. Read-only metadata; no secrets cross the wire.
 *
 * Toggle with `o`. Defaults to expanded on first load so the operator
 * sees it.
 */

import { useEffect, useState } from 'react'

import { ARC_SIGNER_URL, useEnclaveHealth } from '@/lib/arc/enclave'

export function OperatorOverlay() {
  const { data, reachable, lastChecked } = useEnclaveHealth()
  const [open, setOpen] = useState(true)

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== 'o' || e.metaKey || e.ctrlKey || e.altKey) return
      const t = e.target as HTMLElement | null
      if (t && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA')) return
      setOpen((v) => !v)
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [])

  return (
    <div
      data-testid="operator-overlay"
      className="fixed right-4 bottom-4 z-50 font-mono text-xs"
      style={{
        background: 'var(--bg-base)',
        border: '1px solid var(--border-default)',
        color: 'var(--text-primary)',
        minWidth: open ? 360 : 'auto',
      }}
    >
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        className="flex items-center gap-3 w-full px-4 py-2 text-left"
      >
        <span
          className="inline-block h-1.5 w-1.5 rounded-full"
          style={{
            background: reachable ? 'var(--solana-mint)' : 'var(--danger)',
            boxShadow: reachable ? '0 0 6px rgba(20,241,149,0.6)' : 'none',
          }}
        />
        <span
          className="uppercase text-[10px]"
          style={{ color: 'var(--brand-blue)', letterSpacing: '0.25em' }}
        >
          Operator · {reachable ? 'Enclave live' : 'Offline'}
        </span>
        <span className="ml-auto" style={{ color: 'var(--text-tertiary)' }}>[o]</span>
      </button>

      {open && (
        <div
          className="px-4 pb-4 pt-2 space-y-2 border-t"
          style={{ borderColor: 'var(--border-subtle)' }}
        >
          {reachable && data ? (
            <>
              <Row label="bind">{data.bind_addr}</Row>
              <Row label="stress mode">
                <span style={{ color: data.stress_mode ? 'var(--solana-mint)' : 'var(--text-secondary)' }}>
                  {data.stress_mode ? 'on' : 'off'}
                </span>
              </Row>
              <Row label="pending">{data.pending}</Row>
              <Row label="keys">
                <span className="flex flex-wrap gap-2">
                  {data.keys_loaded.length === 0 && (
                    <span style={{ color: 'var(--danger)' }}>none</span>
                  )}
                  {data.keys_loaded.map((k) => (
                    <span
                      key={k}
                      className="px-2 py-0.5 text-[10px]"
                      style={{
                        background: 'var(--brand-blue-soft)',
                        color: 'var(--brand-blue)',
                        border: '1px solid rgba(61,165,255,0.2)',
                      }}
                    >
                      {k}
                    </span>
                  ))}
                </span>
              </Row>
              <Row label="devnets">
                <span style={{ color: 'var(--text-secondary)' }}>
                  {data.devnet_chains.join(', ')}
                </span>
              </Row>
              {lastChecked && (
                <Row label="checked">
                  <span style={{ color: 'var(--text-tertiary)' }}>
                    {new Date(lastChecked).toLocaleTimeString()}
                  </span>
                </Row>
              )}
            </>
          ) : (
            <div style={{ color: 'var(--text-tertiary)' }} className="py-2">
              cannot reach enclave at {ARC_SIGNER_URL}
            </div>
          )}
        </div>
      )}
    </div>
  )
}

function Row({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="flex items-baseline gap-3">
      <span
        className="uppercase text-[10px] w-24 shrink-0"
        style={{ color: 'var(--text-tertiary)', letterSpacing: '0.2em' }}
      >
        {label}
      </span>
      <span className="text-[11px]">{children}</span>
    </div>
  )
}
