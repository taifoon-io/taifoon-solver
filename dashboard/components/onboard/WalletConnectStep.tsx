'use client'

// WalletConnectStep — the SIWE-driven wallet step of the onboarding wizard.
//
// Flow:
//   1. User clicks Connect → wagmi opens the injected wallet (or shows the
//      WalletConnect modal if a WC project id is configured).
//   2. We fetch a server-issued nonce from /api/hosting/siwe-nonce.
//   3. We build a SIWE message with the documented statement, the user's
//      address, the issued nonce, and a 5-minute expiration.
//   4. The wallet signs the message via `useSignMessage`. Errors (user
//      rejected, network issue, nonce expired) surface inline.
//   5. The signed message + signature are forwarded to the parent via
//      `onSiweReady` so the existing Step 3 → /api/hosting/provision flow
//      can include them in the body.

import { useEffect, useState } from 'react'
import {
  useAccount,
  useConnect,
  useDisconnect,
  useSignMessage,
} from 'wagmi'
import { SiweMessage } from 'siwe'
import { SIWE_CHAIN_ID, SIWE_DOMAIN } from '@/lib/wagmi'

export interface SiweArtifacts {
  message: string
  signature: string
  address: `0x${string}`
}

interface WalletConnectStepProps {
  onSiweReady: (artifacts: SiweArtifacts | null) => void
  /** Existing/known artifacts so we can resume mid-wizard without re-signing. */
  existing?: SiweArtifacts | null
}

const STATEMENT =
  'Sign to provision a Taifoon solver pod. This signature is used to prove address ownership and is not a transaction. No funds are moved.'

export function WalletConnectStep({ onSiweReady, existing }: WalletConnectStepProps) {
  const { address, isConnected } = useAccount()
  const { connectors, connect, isPending: connecting, error: connectError } = useConnect()
  const { disconnect } = useDisconnect()
  const { signMessageAsync, isPending: signing } = useSignMessage()

  const [siwe, setSiwe] = useState<SiweArtifacts | null>(existing ?? null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  // If the user disconnects, blow away any stale SIWE artifact — re-sign required.
  useEffect(() => {
    if (!isConnected && siwe) {
      setSiwe(null)
      onSiweReady(null)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [isConnected])

  async function handleSign() {
    if (!address) {
      setError('Connect a wallet first.')
      return
    }
    setBusy(true)
    setError(null)
    try {
      // 1) Get a fresh nonce — single-use, scoped to this address, 5 min TTL.
      const nonceRes = await fetch('/api/hosting/siwe-nonce', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ address: address.toLowerCase() }),
      })
      if (!nonceRes.ok) {
        const e = await nonceRes.json().catch(() => ({}))
        throw new Error(e.error || `Nonce request failed (HTTP ${nonceRes.status})`)
      }
      const { nonce } = (await nonceRes.json()) as { nonce: string }

      // 2) Build SIWE message. The siwe lib emits the canonical EIP-4361
      // text exactly the way the Rust verifier expects to parse it.
      const expirationTime = new Date(Date.now() + 5 * 60 * 1000).toISOString()
      const message = new SiweMessage({
        domain: SIWE_DOMAIN,
        address,
        statement: STATEMENT,
        uri: `https://${SIWE_DOMAIN}`,
        version: '1',
        chainId: SIWE_CHAIN_ID,
        nonce,
        expirationTime,
        issuedAt: new Date().toISOString(),
      })
      const prepared = message.prepareMessage()

      // 3) Sign with the connected wallet (personal_sign under the hood).
      const signature = await signMessageAsync({ message: prepared })

      const artifacts: SiweArtifacts = { message: prepared, signature, address }
      setSiwe(artifacts)
      onSiweReady(artifacts)
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e)
      // Common case: user rejected the signature in the wallet popup.
      if (/reject|denied|user/i.test(msg)) {
        setError('Signature rejected. Click "Sign to verify" again when ready.')
      } else if (/nonce/i.test(msg)) {
        setError('Nonce expired — please click "Sign to verify" again to get a fresh nonce.')
      } else {
        setError(msg)
      }
      setSiwe(null)
      onSiweReady(null)
    } finally {
      setBusy(false)
    }
  }

  if (!isConnected) {
    return (
      <div className="space-y-4">
        <div className="rounded-[var(--r-md)] border border-[var(--brand-cyan)]/20 bg-[var(--brand-cyan)]/5 px-4 py-3">
          <div className="text-[10px] uppercase tracking-[0.18em] text-[var(--brand-cyan)] font-bold mb-1">
            Step 1 · Connect wallet
          </div>
          <p className="text-[12px] text-[var(--text-secondary)] leading-relaxed">
            Your wallet address receives 70% of every fill&apos;s TSUL donut.
            Taifoon never holds your key.
          </p>
        </div>

        <div className="flex flex-wrap gap-2">
          {connectors.map((c) => (
            <button
              key={c.uid}
              onClick={() => connect({ connector: c })}
              disabled={connecting}
              className="h-11 px-5 rounded-[var(--r-md)] border border-[var(--brand-cyan)] bg-[var(--brand-cyan)]/10 text-[var(--brand-cyan)] text-sm font-semibold hover:bg-[var(--brand-cyan)]/15 transition-colors disabled:opacity-60"
            >
              {connecting ? 'Connecting…' : `Connect ${c.name}`}
            </button>
          ))}
        </div>

        <p className="text-[11px] text-[var(--text-tertiary)]">
          Solana wallet coming soon — Spinner provisioning is EVM-only for now.
        </p>

        {connectError && (
          <div className="rounded-[var(--r-md)] border border-[var(--danger)]/30 bg-[var(--danger)]/5 px-4 py-3 text-[12px] text-[var(--danger)] font-mono">
            {connectError.message}
          </div>
        )}
      </div>
    )
  }

  return (
    <div className="space-y-4">
      <div className="rounded-[var(--r-md)] border border-[var(--solana-mint)]/30 bg-[var(--solana-mint)]/5 px-4 py-3 flex items-center justify-between gap-3">
        <div className="min-w-0">
          <div className="text-[10px] uppercase tracking-[0.18em] text-[var(--solana-mint)] font-bold mb-1">
            Connected
          </div>
          <div className="font-mono text-[12px] text-[var(--text-primary)] truncate">
            {address}
          </div>
        </div>
        <button
          onClick={() => disconnect()}
          className="text-[11px] uppercase tracking-[0.18em] text-[var(--text-tertiary)] hover:text-[var(--text-primary)]"
        >
          [ disconnect ]
        </button>
      </div>

      {siwe ? (
        <div className="rounded-[var(--r-md)] border border-[var(--brand-cyan)]/30 bg-[var(--brand-cyan)]/5 px-4 py-3">
          <div className="text-[10px] uppercase tracking-[0.18em] text-[var(--brand-cyan)] font-bold mb-1">
            Wallet verified
          </div>
          <p className="text-[12px] text-[var(--text-secondary)] leading-relaxed">
            SIWE signature captured. Continue to register.
          </p>
        </div>
      ) : (
        <button
          onClick={handleSign}
          disabled={busy || signing}
          className="h-11 px-5 rounded-[var(--r-md)] border border-[var(--brand-cyan)] bg-[var(--brand-cyan)]/10 text-[var(--brand-cyan)] text-sm font-semibold hover:bg-[var(--brand-cyan)]/15 transition-colors disabled:opacity-60"
        >
          {busy || signing ? 'Awaiting signature…' : 'Sign to verify ownership'}
        </button>
      )}

      <p className="text-[11px] text-[var(--text-tertiary)] leading-relaxed">
        Signing is free — no transaction, no gas. We use the signature to
        prove you control this address before we issue your provisioning API
        token.
      </p>

      {error && (
        <div className="rounded-[var(--r-md)] border border-[var(--danger)]/30 bg-[var(--danger)]/5 px-4 py-3 text-[12px] text-[var(--danger)] font-mono">
          {error}
        </div>
      )}
    </div>
  )
}
