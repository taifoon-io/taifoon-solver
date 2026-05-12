'use client'

// ProvisionedSolver — renders the post-provision artifacts: solver_id,
// one-time api_token, install command, keychain command, and a deep link
// to the operator portal. Used by the onboard page after a successful
// POST /api/hosting/provision.

import { useState } from 'react'
import { Card, CardHeader, Snippet } from '@/components/ui'

export interface ProvisionResult {
  solver_id: string
  api_token: string
  portal_url: string
  watch_url: string
  signing_mode: string
  tsul_note: string
}

export function ProvisionedSolver({ result }: { result: ProvisionResult }) {
  const installCmd = `curl -sSf https://solver.taifoon.dev/install.sh | sh`
  // The "<YOUR_KEY>" placeholder is intentional — we never want a real
  // private key wandering through React state, copy/paste history, or
  // server logs. The user substitutes it locally before running.
  const keychainCmd = `security add-generic-password -a "$USER" -s mamba-messiah-key -w "0xYOUR_KEY"`

  return (
    <div className="space-y-4">
      <div className="rounded-[var(--r-md)] border border-[var(--solana-mint)]/40 bg-[var(--solana-mint)]/5 px-5 py-4">
        <div className="text-[var(--solana-mint)] text-sm font-bold mb-1">
          Registered — solver_id: <span className="font-mono">{result.solver_id}</span>
        </div>
        <p className="text-[12px] text-[var(--text-secondary)] leading-relaxed">
          {result.tsul_note}
        </p>
      </div>

      <Card padding="md" className="bg-[var(--bg-raised)]">
        <CardHeader title="Your API token — save this now, not recoverable" />
        <div className="flex items-center gap-2">
          <div className="font-mono text-[11px] bg-black/30 rounded px-3 py-2 break-all text-[var(--brand-cyan)] border border-[var(--border-subtle)] flex-1">
            {result.api_token}
          </div>
          <CopyButton value={result.api_token} />
        </div>
        <p className="mt-2 text-[11px] text-[var(--danger)]">
          Shown once. Lose it and you&apos;ll need to re-provision (re-provisioning rotates the token).
          Use it as <code className="font-mono">SOLVER_API_TOKEN</code> on your solver pod.
        </p>
      </Card>

      <div>
        <div className="text-[11px] uppercase tracking-[0.18em] text-[var(--text-tertiary)] mb-2 font-bold">
          Install command
        </div>
        <Snippet code={installCmd} variant="compact" />
      </div>

      <div>
        <div className="text-[11px] uppercase tracking-[0.18em] text-[var(--text-tertiary)] mb-2 font-bold">
          Store key in macOS Keychain
        </div>
        <Snippet code={keychainCmd} variant="compact" />
        <p className="mt-1.5 text-[11px] text-[var(--text-tertiary)]">
          Replace <code className="font-mono text-[var(--brand-cyan)]">0xYOUR_KEY</code> with your funded EVM private key.
          The solver reads from Keychain at startup — no env var, no shell history.
        </p>
      </div>

      <a
        href={result.portal_url}
        className="block rounded-[var(--r-md)] border border-[var(--brand-blue)]/30 bg-[var(--brand-blue)]/5 px-4 py-3 text-center hover:border-[var(--brand-blue)] transition-all"
      >
        <div className="text-[10px] uppercase tracking-wider text-[var(--text-tertiary)] mb-1">Operator portal</div>
        <div className="font-mono text-[12px] text-[var(--brand-blue)]">
          /portal/{result.solver_id}
        </div>
      </a>
    </div>
  )
}

function CopyButton({ value }: { value: string }) {
  const [copied, setCopied] = useState(false)
  return (
    <button
      onClick={async () => {
        try {
          await navigator.clipboard.writeText(value)
          setCopied(true)
          setTimeout(() => setCopied(false), 1400)
        } catch {
          /* clipboard blocked (e.g. iframe) — silently ignore */
        }
      }}
      className="font-mono text-[10px] tracking-[0.22em] uppercase text-[var(--text-tertiary)] hover:text-[var(--brand-cyan)] transition-colors px-3 py-2 border border-[var(--border-default)] rounded-[var(--r-sm)] shrink-0"
    >
      {copied ? '[ COPIED ]' : '[ COPY ]'}
    </button>
  )
}
