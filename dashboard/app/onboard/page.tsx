'use client'

// Per-route SEO is set in app/onboard/layout.tsx — keeping this file as
// a client component for the wizard interactivity.

import { useEffect, useMemo, useState } from 'react'
import { useRouter } from 'next/navigation'
import { useAccount } from 'wagmi'
import { NavBar, Footer, Card, CardHeader, Stepper, StepBody, Button, Snippet, Tag } from '@/components/ui'
import { protocolColors } from '@/lib/tokens'
import { WalletConnectStep, type SiweArtifacts } from '@/components/onboard/WalletConnectStep'
import { ProvisionedSolver, type ProvisionResult } from '@/components/onboard/ProvisionedSolver'

const STEPS = [
  { label: 'Identity', description: 'Name & operator email' },
  { label: 'Chains & protocols', description: 'What you want to solve' },
  { label: 'Wallet & signing', description: 'Key authority model' },
  { label: 'Launch', description: 'Register & get your portal' },
]

const CHAIN_OPTIONS = [
  { id: 'ethereum', label: 'Ethereum', solana: false },
  { id: 'base', label: 'Base', solana: false },
  { id: 'arbitrum', label: 'Arbitrum', solana: false },
  { id: 'optimism', label: 'Optimism', solana: false },
  { id: 'polygon', label: 'Polygon', solana: false },
  { id: 'bsc', label: 'BSC', solana: false },
  { id: 'avalanche', label: 'Avalanche', solana: false },
  { id: 'solana', label: 'Solana', solana: true },
] as const

const PROTOCOL_OPTIONS = [
  { id: 'across', label: 'Across V3', tier: 1 },
  { id: 'debridge', label: 'deBridge DLN', tier: 1 },
  { id: 'mayan', label: 'Mayan Swift', tier: 1 },
  { id: 'lifi', label: 'LiFi', tier: 1 },
  { id: 'stargate', label: 'Stargate V2', tier: 1 },
  { id: 'cctp', label: 'CCTP', tier: 2 },
  { id: 'wormhole', label: 'Wormhole', tier: 2 },
  { id: 'hop', label: 'Hop', tier: 2 },
  { id: 'connext', label: 'Connext', tier: 2 },
  { id: 'synapse', label: 'Synapse', tier: 2 },
  { id: 'orbiter', label: 'Orbiter', tier: 3 },
  { id: 'celer', label: 'Celer cBridge', tier: 3 },
] as const

type SigningMode = 'self_hosted' | 'remote_signer' | 'session_key'

function isValidEvm(addr: string): boolean {
  return /^0x[0-9a-fA-F]{40}$/.test(addr.trim())
}

function isValidSolana(addr: string): boolean {
  // Base58 pubkey: 32-44 chars, alphanumeric excluding 0, O, I, l
  return /^[1-9A-HJ-NP-Za-km-z]{32,44}$/.test(addr.trim())
}

export default function OnboardPage() {
  const router = useRouter()
  const [step, setStep] = useState(0)

  // Step 0
  const [name, setName] = useState('')
  const [email, setEmail] = useState('')

  // Step 1
  const [chains, setChains] = useState<Set<string>>(new Set(['base', 'solana', 'arbitrum']))
  const [protocols, setProtocols] = useState<Set<string>>(new Set(['across', 'debridge', 'mayan']))

  // Step 2 — key authority
  const [signingMode, setSigningMode] = useState<SigningMode>('self_hosted')
  const [solanaAddress, setSolanaAddress] = useState('')
  const [webhookUrl, setWebhookUrl] = useState('')
  const [safeAddress, setSafeAddress] = useState('')

  // Wallet-derived state — `evmAddress` is now driven by the connected
  // wagmi account, not a manual input field.
  const { address: connectedAddress } = useAccount()
  const evmAddress = connectedAddress ?? ''
  const [siwe, setSiwe] = useState<SiweArtifacts | null>(null)
  const [welcomeBack, setWelcomeBack] = useState<{ solver_id: string } | null>(null)
  const [solanaAddressError, setSolanaAddressError] = useState<string | null>(null)

  // Step 3 — provisioning
  const [provisioning, setProvisioning] = useState(false)
  const [provisionResult, setProvisionResult] = useState<ProvisionResult | null>(null)
  const [provisionError, setProvisionError] = useState<string | null>(null)

  // Welcome-back detection: when the user connects with a wallet that was
  // previously provisioned, we hit GET /api/hosting/solvers/:solver_id to
  // confirm and surface a "you've been here before" hint. We derive the
  // solver_id client-side from the address so we don't need a separate
  // lookup endpoint.
  useEffect(() => {
    if (!connectedAddress) {
      setWelcomeBack(null)
      return
    }
    const solverId = connectedAddress.toLowerCase().replace(/^0x/, '').slice(0, 8)
    let cancelled = false
    fetch(`/api/hosting/solvers/${solverId}`)
      .then((r) => (r.ok ? r.json() : null))
      .then((data) => {
        if (cancelled) return
        if (data && data.solver_id) setWelcomeBack({ solver_id: data.solver_id })
        else setWelcomeBack(null)
      })
      .catch(() => {
        if (!cancelled) setWelcomeBack(null)
      })
    return () => {
      cancelled = true
    }
  }, [connectedAddress])

  const launchCmd = useMemo(() => {
    const addr = evmAddress || '<your_evm_address>'
    const protos = Array.from(protocols).join(',')
    if (signingMode === 'remote_signer') {
      return `# Your solver signs transactions locally — you approve each fill.
# 1. Clone & build
git clone https://github.com/yawningmonsoon/taifoon-solver
cd taifoon-solver && cargo build --release -p solver-main

# 2. Configure
export SOLVER_PRIVATE_KEY=0x<your_funded_evm_key>
export SOLVER_ADDRESS=${addr}
export PROTOCOL_FILTER=${protos}
export SIGNER_MODE=remote
export SIGNER_WEBHOOK_URL=${webhookUrl || 'http://localhost:9000/sign'}

# 3. Run
./target/release/taifoon-solver`
    }
    if (signingMode === 'session_key') {
      return `# You hold the Safe master key — Taifoon holds a scoped session key.
# Session key can ONLY call: fillRelay, fulfillOrder, fulfillSimple.
# Safe spend limit + session key scope enforced at contract level.

# 1. Deploy or connect your Safe: https://safe.global
# 2. Add Taifoon as a signer with module: TaifoonSessionModule
#    (restricts to fill-function selectors only, daily spend cap)
# 3. Your solver runs in Taifoon's hosted fleet:
#    solver_id: ${evmAddress ? evmAddress.slice(2, 10) : '<id>'}
#    portal: /portal/${evmAddress ? evmAddress.slice(2, 10).toLowerCase() : '<id>'}
# 4. Revoke at any time from your Safe interface.`
    }
    // self_hosted
    return `# You run the solver binary on your own machine.
# Taifoon NEVER holds your key. All fills signed locally.

# 1. Clone & build
git clone https://github.com/yawningmonsoon/taifoon-solver
cd taifoon-solver && cargo build --release -p solver-main

# 2. Store key in macOS keychain (recommended)
security add-generic-password -a "$USER" -s mamba-messiah-key \\
  -w "0x<your_funded_evm_key>"

# 3. Run
export SOLVER_ADDRESS=${addr}
export PROTOCOL_FILTER=${protos}
export MIN_PROFIT_USD=0.10
export OUTCOME_DB_PATH=./outcomes/${name || 'my-solver'}_live.sqlite
./target/release/taifoon-solver`
  }, [name, chains, protocols, signingMode, evmAddress, webhookUrl])

  const canAdvance = (() => {
    if (step === 0) return !!name && !!email
    if (step === 1) return chains.size > 0 && protocols.size > 0
    // Step 2: wallet must be connected AND the SIWE signature captured so
    // the server can flip `siwe_verified=1` on the row. If a Solana address
    // is entered it must be valid before allowing advance.
    if (step === 2) return isValidEvm(evmAddress) && !!siwe && (!solanaAddress || isValidSolana(solanaAddress))
    return true
  })()

  async function handleProvision() {
    if (provisionResult) {
      // Already provisioned — just navigate
      router.push(`/portal/${provisionResult.solver_id}`)
      return
    }

    setProvisioning(true)
    setProvisionError(null)

    try {
      const res = await fetch('/api/hosting/provision', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          name,
          evm_address: evmAddress.trim(),
          solana_address: solanaAddress.trim() || undefined,
          signing_mode: signingMode,
          signer_webhook_url: signingMode === 'remote_signer' ? webhookUrl : undefined,
          safe_address: signingMode === 'session_key' ? safeAddress : undefined,
          email: email || undefined,
          chains: Array.from(chains).join(','),
          protocols: Array.from(protocols).join(','),
          // SIWE artifacts — server verifies these before issuing the
          // api_token and flips `siwe_verified=1` on the row.
          siwe_message: siwe?.message,
          signature: siwe?.signature,
        }),
      })

      if (!res.ok) {
        const err = await res.json().catch(() => ({ error: `HTTP ${res.status}` }))
        throw new Error(err.error || `HTTP ${res.status}`)
      }

      const result: ProvisionResult = await res.json()
      setProvisionResult(result)
    } catch (e) {
      setProvisionError(e instanceof Error ? e.message : String(e))
    } finally {
      setProvisioning(false)
    }
  }

  return (
    <>
      <NavBar />
      <main className="flex-1">
        <div className="max-w-[920px] mx-auto px-6 py-12">
          <div className="mb-10">
            <Tag>Onboarding</Tag>
            <h1 className="tf-display tf-gradient-silver mt-4 text-[clamp(2rem,4vw,3rem)]">
              Spin up your solver.
            </h1>
            <p className="mt-3 text-[var(--text-secondary)] max-w-[580px] leading-relaxed">
              Four steps. Under five minutes. You retain full key authority —
              Taifoon never holds your private key. Every fill routes 70% of the
              TSUL donut to your wallet, perpetually.
            </p>
          </div>

          <Card padding="lg">
            <Stepper steps={STEPS} current={step} />

            <StepBody>
              {step === 0 && (
                <div className="space-y-5">
                  <Field label="Solver name" hint="Shown in the fleet portal. e.g. colosseum-prod-01">
                    <input
                      autoFocus
                      value={name}
                      onChange={(e) => setName(e.target.value)}
                      placeholder="my-solver"
                      className="w-full bg-[var(--bg-raised)] border border-[var(--border-default)] rounded-[var(--r-md)] px-4 h-11 text-sm focus:border-[var(--brand-cyan)] outline-none"
                    />
                  </Field>
                  <Field label="Operator email" hint="Registration receipt and fill alerts.">
                    <input
                      type="email"
                      value={email}
                      onChange={(e) => setEmail(e.target.value)}
                      placeholder="you@team.xyz"
                      className="w-full bg-[var(--bg-raised)] border border-[var(--border-default)] rounded-[var(--r-md)] px-4 h-11 text-sm focus:border-[var(--brand-cyan)] outline-none"
                    />
                  </Field>
                </div>
              )}

              {step === 1 && (
                <div className="space-y-7">
                  <div>
                    <SectionLabel>Chains</SectionLabel>
                    <div className="grid grid-cols-2 sm:grid-cols-4 gap-2">
                      {CHAIN_OPTIONS.map((c) => {
                        const active = chains.has(c.id)
                        return (
                          <button
                            key={c.id}
                            onClick={() =>
                              setChains((prev) => {
                                const next = new Set(prev)
                                if (next.has(c.id)) next.delete(c.id)
                                else next.add(c.id)
                                return next
                              })
                            }
                            className={`px-3 py-3 rounded-[var(--r-md)] border text-left transition-all ${
                              active
                                ? c.solana
                                  ? 'border-[var(--brand-violet)] bg-[var(--brand-violet)]/10'
                                  : 'border-[var(--brand-cyan)] bg-[var(--brand-cyan)]/10'
                                : 'border-[var(--border-default)] bg-[var(--bg-raised)] hover:border-[var(--border-strong)]'
                            }`}
                          >
                            <div
                              className={`text-sm font-semibold ${
                                active && c.solana
                                  ? 'text-[var(--brand-violet)]'
                                  : active
                                    ? 'text-[var(--brand-cyan)]'
                                    : 'text-[var(--text-primary)]'
                              }`}
                            >
                              {c.label}
                            </div>
                            <div className="text-[10px] text-[var(--text-tertiary)] uppercase tracking-wider mt-1">
                              {c.solana ? 'svm' : 'evm'}
                            </div>
                          </button>
                        )
                      })}
                    </div>
                  </div>

                  <div>
                    <SectionLabel>Protocols</SectionLabel>
                    <div className="flex flex-wrap gap-2">
                      {PROTOCOL_OPTIONS.map((p) => {
                        const active = protocols.has(p.id)
                        const color = protocolColors[p.id] ?? '#A0A0B0'
                        return (
                          <button
                            key={p.id}
                            onClick={() =>
                              setProtocols((prev) => {
                                const next = new Set(prev)
                                if (next.has(p.id)) next.delete(p.id)
                                else next.add(p.id)
                                return next
                              })
                            }
                            className="px-3 h-9 rounded-[var(--r-pill)] border text-[12px] font-semibold transition-all"
                            style={{
                              borderColor: active ? color : 'var(--border-default)',
                              background: active ? `${color}1f` : 'var(--bg-raised)',
                              color: active ? color : 'var(--text-secondary)',
                            }}
                          >
                            {p.label}
                            {p.tier === 1 && (
                              <span
                                className="ml-2 text-[9px] uppercase tracking-wider"
                                style={{ color: active ? color : 'var(--text-tertiary)' }}
                              >
                                tier1
                              </span>
                            )}
                          </button>
                        )
                      })}
                    </div>
                    <p className="mt-3 text-[11px] text-[var(--text-tertiary)]">
                      {protocols.size} protocol{protocols.size === 1 ? '' : 's'} · {chains.size} chain
                      {chains.size === 1 ? '' : 's'} selected. You can change these later.
                    </p>
                  </div>
                </div>
              )}

              {step === 2 && (
                <div className="space-y-6">
                  {/* Key authority model */}
                  <div>
                    <SectionLabel>Signing mode — Taifoon never holds your private key</SectionLabel>
                    <div className="grid grid-cols-1 sm:grid-cols-3 gap-3">
                      <ModeCard
                        active={signingMode === 'self_hosted'}
                        onClick={() => setSigningMode('self_hosted')}
                        title="Self-hosted"
                        description="Run the solver binary on your own machine. You sign all fills locally. Full custody."
                        tone="cyan"
                        badge="Recommended"
                      />
                      <ModeCard
                        active={signingMode === 'remote_signer'}
                        onClick={() => setSigningMode('remote_signer')}
                        title="Remote signer"
                        description="Taifoon builds unsigned transactions. Your local webhook approves each fill. Non-custodial."
                        tone="violet"
                        badge="Non-custodial"
                      />
                      <ModeCard
                        active={signingMode === 'session_key'}
                        onClick={() => setSigningMode('session_key')}
                        title="Safe session key"
                        description="Deploy a Safe multisig. Grant Taifoon a scoped session key (fill-functions only, spend cap)."
                        tone="cyan"
                        badge="DeFi-native"
                      />
                    </div>
                  </div>

                  {/* Wallet connect + SIWE — replaces manual address entry. */}
                  <div>
                    <div className="text-[11px] uppercase tracking-[0.18em] text-[var(--text-tertiary)] mb-3 font-bold">
                      Connect & verify
                    </div>
                    <WalletConnectStep
                      onSiweReady={setSiwe}
                      existing={siwe}
                    />
                    {welcomeBack && (
                      <div className="mt-3 rounded-[var(--r-md)] border border-[var(--brand-blue)]/30 bg-[var(--brand-blue)]/5 px-4 py-3">
                        <div className="text-[10px] uppercase tracking-[0.18em] text-[var(--brand-blue)] font-bold mb-1">
                          Welcome back
                        </div>
                        <p className="text-[12px] text-[var(--text-secondary)] leading-relaxed">
                          This wallet is already provisioned as{' '}
                          <code className="font-mono text-[var(--brand-blue)]">{welcomeBack.solver_id}</code>.
                          Re-provisioning will <strong>rotate your API token</strong> (the old one stops working).
                        </p>
                      </div>
                    )}
                  </div>

                  {/* Solana address — optional */}
                  <Field
                    label="Solana address (optional)"
                    hint="Required only for Mayan Swift Solana-side fills."
                  >
                    <input
                      value={solanaAddress}
                      onChange={(e) => {
                        setSolanaAddress(e.target.value)
                        setSolanaAddressError(null)
                      }}
                      onBlur={() => {
                        if (solanaAddress && !isValidSolana(solanaAddress))
                          setSolanaAddressError('Must be a valid base58 Solana address (32–44 chars, no 0/O/I/l)')
                      }}
                      placeholder="Base58 pubkey…"
                      spellCheck={false}
                      autoComplete="off"
                      className="w-full bg-[var(--bg-raised)] border border-[var(--border-default)] rounded-[var(--r-md)] px-4 h-11 text-sm font-mono focus:border-[var(--brand-cyan)] outline-none"
                    />
                    {solanaAddressError && (
                      <p className="mt-1 text-[11px] text-[var(--danger)]">{solanaAddressError}</p>
                    )}
                  </Field>

                  {/* Mode-specific fields */}
                  {signingMode === 'remote_signer' && (
                    <Field
                      label="Signer webhook URL"
                      hint="Your local signer endpoint. Receives POST {calldata, to, value} → returns {signature}."
                    >
                      <input
                        value={webhookUrl}
                        onChange={(e) => setWebhookUrl(e.target.value)}
                        placeholder="http://localhost:9000/sign"
                        spellCheck={false}
                        className="w-full bg-[var(--bg-raised)] border border-[var(--border-default)] rounded-[var(--r-md)] px-4 h-11 text-sm font-mono focus:border-[var(--brand-violet)] outline-none"
                      />
                    </Field>
                  )}

                  {signingMode === 'session_key' && (
                    <Field
                      label="Safe address"
                      hint="The Safe multisig you control. You add Taifoon's session key as a module with restricted selectors."
                    >
                      <input
                        value={safeAddress}
                        onChange={(e) => setSafeAddress(e.target.value)}
                        placeholder="0x… Safe address"
                        spellCheck={false}
                        className="w-full bg-[var(--bg-raised)] border border-[var(--border-default)] rounded-[var(--r-md)] px-4 h-11 text-sm font-mono focus:border-[var(--brand-cyan)] outline-none"
                      />
                    </Field>
                  )}

                  {/* TSUL info box */}
                  <div className="rounded-[var(--r-md)] border border-[var(--solana-mint)]/20 bg-[var(--solana-mint)]/5 px-4 py-3">
                    <div className="text-[10px] uppercase tracking-[0.18em] text-[var(--solana-mint)] mb-1.5 font-bold">
                      TSUL Rule #4
                    </div>
                    <p className="text-[12px] text-[var(--text-secondary)] leading-relaxed">
                      From the moment your address is registered, every settled fill emits
                      a signed donut attestation that redistributes the adapter-owner
                      inflow 70 / 20 / 10 across <code className="font-mono text-[var(--brand-cyan)]">adapter_builder</code>,{' '}
                      <code className="font-mono text-[var(--brand-cyan)]">adapter_reviewers</code>,{' '}
                      <code className="font-mono text-[var(--brand-cyan)]">adapter_ecosystem</code>.
                      Perpetual. Irrevocable. Append-only ledger.
                    </p>
                  </div>
                </div>
              )}

              {step === 3 && (
                <div className="space-y-5">
                  {!provisionResult ? (
                    <>
                      <div className="grid grid-cols-3 gap-3">
                        <Summary label="Solver" value={name || '—'} />
                        <Summary label="Chains" value={Array.from(chains).join(', ')} />
                        <Summary label="Protocols" value={`${protocols.size} selected`} />
                      </div>
                      <div className="grid grid-cols-2 gap-3">
                        <Summary label="EVM Address" value={evmAddress ? `${evmAddress.slice(0, 10)}…${evmAddress.slice(-6)}` : '—'} />
                        <Summary label="Signing Mode" value={signingMode.replace('_', ' ')} />
                      </div>

                      <Snippet code={launchCmd} lang="bash" />

                      {signingMode !== 'self_hosted' && (
                        <Card padding="md" className="bg-[var(--bg-raised)]">
                          <CardHeader title="What happens when you click Register" />
                          <ol className="space-y-2 text-sm text-[var(--text-secondary)] list-none pl-0">
                            <Step n={1}>
                              Your address is registered in the Taifoon fleet. Your{' '}
                              <code className="font-mono text-[var(--brand-cyan)]">solver_id</code>{' '}
                              is the first 8 hex chars of your address.
                            </Step>
                            <Step n={2}>
                              You get a one-time API token for your portal. Save it — it won&apos;t be shown again.
                            </Step>
                            <Step n={3}>
                              TSUL donut routing activates: every fill tied to your address credits 70% to you on-chain.
                            </Step>
                          </ol>
                        </Card>
                      )}

                      {signingMode === 'self_hosted' && (
                        <Card padding="md" className="bg-[var(--bg-raised)]">
                          <CardHeader title="Self-hosted: your machine, your key" />
                          <ol className="space-y-2 text-sm text-[var(--text-secondary)] list-none pl-0">
                            <Step n={1}>
                              Your private key never leaves your machine. Store it in macOS Keychain
                              or pass via <code className="font-mono text-[var(--brand-cyan)]">SOLVER_PRIVATE_KEY</code>.
                            </Step>
                            <Step n={2}>
                              Register below to join the fleet dashboard and activate TSUL donut routing for your address.
                            </Step>
                            <Step n={3}>
                              Run the command above. Your portal goes live as soon as the first intent is detected.
                            </Step>
                          </ol>
                        </Card>
                      )}

                      {provisionError && (
                        <div className="rounded-[var(--r-md)] border border-[var(--danger)]/30 bg-[var(--danger)]/5 px-4 py-3 text-[12px] text-[var(--danger)] font-mono">
                          {provisionError}
                        </div>
                      )}
                    </>
                  ) : (
                    /* Post-provision success screen — solver_id, one-time
                       api_token, install + keychain commands, portal link. */
                    <div className="space-y-4">
                      <ProvisionedSolver result={provisionResult} />
                      <Snippet code={launchCmd} lang="bash" />
                    </div>
                  )}
                </div>
              )}
            </StepBody>

            <div className="mt-10 pt-6 border-t border-[var(--border-subtle)] flex items-center justify-between">
              <Button
                variant="ghost"
                size="md"
                disabled={step === 0}
                onClick={() => setStep((s) => Math.max(0, s - 1))}
              >
                ← Back
              </Button>
              {step < STEPS.length - 1 ? (
                <Button
                  variant="primary"
                  disabled={!canAdvance}
                  onClick={() => setStep((s) => s + 1)}
                >
                  Continue →
                </Button>
              ) : provisionResult ? (
                <Button
                  variant="mint"
                  size="md"
                  onClick={() => router.push(`/portal/${provisionResult.solver_id}`)}
                >
                  Open my portal →
                </Button>
              ) : (
                <Button
                  variant="mint"
                  size="md"
                  disabled={provisioning || !canAdvance}
                  onClick={handleProvision}
                >
                  {provisioning ? 'Registering…' : 'Register & launch →'}
                </Button>
              )}
            </div>
          </Card>

          <p className="mt-6 text-center text-xs text-[var(--text-tertiary)]">
            Need help?{' '}
            <a href="mailto:hello@taifoon.dev" className="text-[var(--brand-cyan)] hover:underline">
              hello@taifoon.dev
            </a>{' '}
            · DM{' '}
            <a
              href="https://github.com/yawningmonsoon"
              target="_blank"
              rel="noreferrer"
              className="text-[var(--brand-cyan)] hover:underline ml-1"
            >
              yawningmonsoon
            </a>
          </p>
          <div className="mt-4 flex justify-center">
            <Button href="/policy" variant="ghost" size="sm">
              Verify the TSUL policy →
            </Button>
          </div>
        </div>
      </main>
      <Footer />
    </>
  )
}

function Field({
  label,
  hint,
  children,
}: {
  label: string
  hint?: string
  children: React.ReactNode
}) {
  return (
    <label className="block">
      <div className="text-[11px] uppercase tracking-[0.18em] text-[var(--text-tertiary)] mb-2 font-bold">
        {label}
      </div>
      {children}
      {hint && <div className="mt-1.5 text-[11px] text-[var(--text-tertiary)]">{hint}</div>}
    </label>
  )
}

function SectionLabel({ children }: { children: React.ReactNode }) {
  return (
    <div className="text-[11px] uppercase tracking-[0.18em] text-[var(--text-tertiary)] mb-3 font-bold">
      {children}
    </div>
  )
}

function ModeCard({
  active,
  onClick,
  title,
  description,
  tone,
  badge,
}: {
  active: boolean
  onClick: () => void
  title: string
  description: string
  tone: 'cyan' | 'violet'
  badge?: string
}) {
  const c = tone === 'cyan' ? 'var(--brand-cyan)' : 'var(--brand-violet)'
  const hex = tone === 'cyan' ? '#00D9FF' : '#9945FF'
  return (
    <button
      onClick={onClick}
      className={`text-left p-4 rounded-[var(--r-lg)] border transition-all relative ${
        active ? '' : 'border-[var(--border-default)] bg-[var(--bg-raised)] hover:border-[var(--border-strong)]'
      }`}
      style={active ? { borderColor: c, background: `${hex}10` } : undefined}
    >
      {badge && (
        <div
          className="absolute top-2 right-2 text-[8px] uppercase tracking-wider px-1.5 py-0.5 rounded"
          style={{ color: c, background: `${hex}20` }}
        >
          {badge}
        </div>
      )}
      <div className="text-sm font-bold mb-1" style={{ color: active ? c : 'var(--text-primary)' }}>
        {title}
      </div>
      <div className="text-[11px] text-[var(--text-secondary)] leading-relaxed">{description}</div>
    </button>
  )
}

function Summary({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-[var(--r-md)] border border-[var(--border-default)] bg-[var(--bg-raised)] px-4 py-3">
      <div className="text-[10px] uppercase tracking-[0.18em] text-[var(--text-tertiary)]">{label}</div>
      <div className="mt-1 text-sm text-[var(--text-primary)] truncate">{value}</div>
    </div>
  )
}

function Step({ n, children }: { n: number; children: React.ReactNode }) {
  return (
    <li className="flex gap-3">
      <span className="shrink-0 w-5 h-5 rounded-full border border-[var(--brand-cyan)] text-[var(--brand-cyan)] text-[10px] font-mono font-bold flex items-center justify-center mt-0.5">
        {n}
      </span>
      <span>{children}</span>
    </li>
  )
}
