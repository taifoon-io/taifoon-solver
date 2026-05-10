'use client'

// Per-route SEO is set in app/onboard/layout.tsx — keeping this file as
// a client component for the wizard interactivity.

import { useEffect, useMemo, useState } from 'react'
import { useRouter } from 'next/navigation'
import { NavBar, Footer, Card, CardHeader, Stepper, StepBody, Button, Snippet, Tag } from '@/components/ui'
import { protocolColors } from '@/lib/tokens'

const STEPS = [
  { label: 'Identity', description: 'Name & operator email' },
  { label: 'Chains & protocols', description: 'What you want to solve' },
  { label: 'Wallet', description: 'Generate or import' },
  { label: 'Launch', description: 'Copy the run command' },
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

export default function OnboardPage() {
  const router = useRouter()
  const [step, setStep] = useState(0)

  // Step 0
  const [name, setName] = useState('')
  const [email, setEmail] = useState('')

  // Step 1
  const [chains, setChains] = useState<Set<string>>(new Set(['base', 'solana', 'arbitrum']))
  const [protocols, setProtocols] = useState<Set<string>>(new Set(['across', 'debridge', 'mayan']))

  // Step 2
  const [walletMode, setWalletMode] = useState<'generate' | 'import'>('generate')
  const [importKey, setImportKey] = useState('')
  const [generatedWallet, setGeneratedWallet] = useState({
    address: '0x————————————————————',
    solana: '—————————————————',
  })
  const [solverId, setSolverId] = useState('spinr_pending')

  // Random/preview values are computed client-side after mount to keep the
  // render pure and avoid SSR/CSR hydration mismatches. The setState calls
  // are intentional post-mount initialization, not synchronization.
  useEffect(() => {
    queueMicrotask(() => {
      setGeneratedWallet({
        address: '0x' + Math.random().toString(16).slice(2, 10) + 'b3e9a2c4d5...',
        solana: 'Sol' + Math.random().toString(36).slice(2, 8) + '...DLN',
      })
      setSolverId('spinr_' + Math.random().toString(36).slice(2, 8))
    })
  }, [])

  const launchCmd = useMemo(() => {
    return `# 1. Clone the solver runtime
git clone https://github.com/yawningmonsoon/taifoon-solver
cd taifoon-solver

# 2. Configure your solver
export SOLVER_PRIVATE_KEY=0x<your_funded_evm_key>
export SOLVER_ADDRESS=<your_evm_address>
export PROTOCOL_FILTER=${Array.from(protocols).join(',')}
export MIN_PROFIT_USD=0.10
export OUTCOME_DB_PATH=./outcomes/${name || 'my-solver'}_live.sqlite

# 3. Build & run (Docker)
docker compose up -d

# — OR — run directly with cargo:
cargo run -p solver-main --release`
  }, [name, chains, protocols, walletMode, solverId])

  const canAdvance = (() => {
    if (step === 0) return !!name && !!email
    if (step === 1) return chains.size > 0 && protocols.size > 0
    if (step === 2) return walletMode === 'generate' || importKey.length > 12
    return true
  })()

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
            <p className="mt-3 text-[var(--text-secondary)] max-w-[560px] leading-relaxed">
              Four phases. About five minutes. End state: a registered
              solver pod, on-chain on Base + Solana, that you can monitor
              live in the portal.
            </p>
          </div>

          <Card padding="lg">
            <Stepper steps={STEPS} current={step} />

            <StepBody>
              {step === 0 && (
                <div className="space-y-5">
                  <Field label="Solver name" hint="Shown in the portal. e.g. colosseum-prod-01">
                    <input
                      autoFocus
                      value={name}
                      onChange={(e) => setName(e.target.value)}
                      placeholder="my-solver"
                      className="w-full bg-[var(--bg-raised)] border border-[var(--border-default)] rounded-[var(--r-md)] px-4 h-11 text-sm focus:border-[var(--brand-cyan)] outline-none"
                    />
                  </Field>
                  <Field label="Operator email" hint="We'll send your registration receipt and ops alerts here.">
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
                      Selected {protocols.size} protocol{protocols.size === 1 ? '' : 's'}, {chains.size} chain
                      {chains.size === 1 ? '' : 's'}. You can change these later.
                    </p>
                  </div>
                </div>
              )}

              {step === 2 && (
                <div className="space-y-5">
                  <div className="grid grid-cols-2 gap-3">
                    <ModeCard
                      active={walletMode === 'generate'}
                      onClick={() => setWalletMode('generate')}
                      title="Generate"
                      description="A fresh keypair is created locally. Your seed is written to ~/.taifoon/solver.toml."
                      tone="cyan"
                    />
                    <ModeCard
                      active={walletMode === 'import'}
                      onClick={() => setWalletMode('import')}
                      title="Import"
                      description="Paste an existing private key. Useful when re-onboarding an existing solver."
                      tone="violet"
                    />
                  </div>

                  {walletMode === 'generate' && (
                    <Card padding="md" className="bg-[var(--bg-raised)]">
                      <CardHeader title="Preview address" />
                      <div className="space-y-2 font-mono text-xs">
                        <div className="flex justify-between gap-4">
                          <span className="text-[var(--text-tertiary)]">EVM</span>
                          <span className="text-[var(--brand-cyan)] truncate">{generatedWallet.address}</span>
                        </div>
                        <div className="flex justify-between gap-4">
                          <span className="text-[var(--text-tertiary)]">Solana</span>
                          <span className="text-[var(--brand-violet)] truncate">{generatedWallet.solana}</span>
                        </div>
                      </div>
                      <p className="mt-3 text-[11px] text-[var(--text-tertiary)]">
                        This is a preview only. Your real wallet is generated on-device when you run the CLI.
                      </p>
                    </Card>
                  )}

                  {walletMode === 'import' && (
                    <Field
                      label="Private key"
                      hint="Hex EVM key or Solana base58 — never leaves your machine."
                    >
                      <input
                        type="password"
                        value={importKey}
                        onChange={(e) => setImportKey(e.target.value)}
                        placeholder="0x… or 5J…"
                        className="w-full bg-[var(--bg-raised)] border border-[var(--border-default)] rounded-[var(--r-md)] px-4 h-11 text-sm font-mono focus:border-[var(--brand-cyan)] outline-none"
                      />
                    </Field>
                  )}
                </div>
              )}

              {step === 3 && (
                <div className="space-y-5">
                  <div className="grid grid-cols-3 gap-3">
                    <Summary label="Solver" value={name || '—'} />
                    <Summary label="Chains" value={Array.from(chains).join(', ')} />
                    <Summary label="Protocols" value={`${protocols.size} selected`} />
                  </div>
                  <Snippet code={launchCmd} lang="bash" />
                  <Card padding="md" className="bg-[var(--bg-raised)]">
                    <CardHeader title="What happens when you run this" />
                    <ol className="space-y-2 text-sm text-[var(--text-secondary)] list-none pl-0">
                      <Step n={1}>
                        The solver binary starts and connects to the Genome SSE intent stream.
                        Your EVM keypair signs fill transactions on-chain.
                      </Step>
                      <Step n={2}>
                        The built-in API server starts on port 8082. Point your dashboard to{' '}
                        <code className="font-mono text-[var(--brand-cyan)]">SOLVER_API_INTERNAL_URL=http://localhost:8082</code>.
                      </Step>
                      <Step n={3}>
                        Open your portal at{' '}
                        <code className="font-mono text-[var(--brand-cyan)]">/portal/&lt;your-address&gt;</code>{' '}
                        to watch the live intent stream, fills, and P&amp;L.
                      </Step>
                    </ol>
                  </Card>
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
                ← BACK
              </Button>
              {step < STEPS.length - 1 ? (
                <Button
                  variant="primary"
                  disabled={!canAdvance}
                  onClick={() => setStep((s) => s + 1)}
                >
                  CONTINUE →
                </Button>
              ) : (
                <Button
                  variant="mint"
                  size="md"
                  onClick={() => router.push('/portal/19b3d79a')}
                >
                  VIEW LIVE SOLVER →
                </Button>
              )}
            </div>
          </Card>

          <p className="mt-6 text-center text-xs text-[var(--text-tertiary)]">
            Need help? <a href="mailto:hello@taifoon.dev" className="text-[var(--brand-cyan)] hover:underline">hello@taifoon.dev</a> · or DM
            <a href="https://github.com/yawningmonsoon" target="_blank" rel="noreferrer" className="text-[var(--brand-cyan)] hover:underline ml-1">
              yawningmonsoon
            </a>
          </p>
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
}: {
  active: boolean
  onClick: () => void
  title: string
  description: string
  tone: 'cyan' | 'violet'
}) {
  const c = tone === 'cyan' ? 'var(--brand-cyan)' : 'var(--brand-violet)'
  return (
    <button
      onClick={onClick}
      className={`text-left p-5 rounded-[var(--r-lg)] border transition-all ${
        active ? '' : 'border-[var(--border-default)] bg-[var(--bg-raised)] hover:border-[var(--border-strong)]'
      }`}
      style={
        active
          ? { borderColor: c, background: `${c === 'var(--brand-cyan)' ? '#00D9FF' : '#9945FF'}10` }
          : undefined
      }
    >
      <div
        className="text-base font-bold mb-1"
        style={{ color: active ? c : 'var(--text-primary)' }}
      >
        {title}
      </div>
      <div className="text-xs text-[var(--text-secondary)] leading-relaxed">{description}</div>
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
