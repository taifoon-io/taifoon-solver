'use client'

import {
  NavBar,
  Footer,
  Card,
  Button,
  Snippet,
  Tag,
  PhaseLabel,
  NewsBand,
} from '@/components/ui'
import { ChainOrbits } from '@/components/marketing/ChainOrbits'
import { LiveTicker } from '@/components/marketing/LiveTicker'
import { CrossChainVolume } from '@/components/marketing/CrossChainVolume'
import { protocolColors } from '@/lib/tokens'

const PROTOCOLS = [
  'Across V3', 'deBridge DLN', 'Mayan Swift', 'LiFi', 'Stargate V2', 'CCTP',
  'Hop', 'Connext', 'Synapse', 'Celer cBridge', 'Wormhole', 'LayerZero v2',
  'Hyperlane', 'Axelar GMP', 'CCIP', 'Socket', 'Squid Router', 'Rango',
  'Symbiosis', 'Meson', 'Allbridge', 'Router Protocol', 'Orbiter',
  'Relay', 'Arbitrum native', 'Optimism native',
  '1inch Fusion', 'Uniswap v4', 'Bancor', 'Wormhole NTT',
] as const

export default function LandingPage() {
  return (
    <>
      <NewsBand />
      <NavBar />
      <main className="flex-1">
        <Hero />
        <Phases />
        <LiveData />
        <Install />
      </main>
      <Footer />
    </>
  )
}

// ── Hero — calm, specific, no theater ────────────────────────────────────
function Hero() {
  return (
    <section
      aria-label="Hero"
      className="relative overflow-hidden min-h-[720px] flex items-center"
    >
      <ChainOrbits />
      {/* legibility vignette — moved here from ChainOrbits so the geometry
          component stays pure and the hero owns its own readability */}
      <div
        className="absolute inset-0 pointer-events-none"
        style={{
          background:
            'radial-gradient(ellipse at center, transparent 25%, rgba(0,0,0,0.88) 80%)',
        }}
      />

      <div className="relative w-full max-w-[1280px] mx-auto px-6 py-24 grid lg:grid-cols-[1.1fr_1fr] gap-12 items-center">
        {/* Left — copy */}
        <div>
          <div className="inline-flex items-center gap-3 font-mono text-[11px] tracking-[0.25em] text-[var(--text-secondary)] mb-8">
            <span className="w-1.5 h-1.5 rounded-full bg-[var(--solana-mint)] pulse-live" />
            taifoon · solvers · solana + evm
          </div>

          <h1
            className="tf-display tf-gradient-silver"
            style={{ fontSize: 'clamp(2.75rem, 6.5vw, 5.5rem)' }}
          >
            The bridge-solver
            <br />
            runtime.
          </h1>

          <p className="mt-8 text-[var(--text-secondary)] max-w-[560px] leading-[1.7] text-[16px]">
            A single Rust binary that speaks 31 cross-chain protocols
            across Solana and EVM. Anchor IDLs, JITO bundles, EIP-1559,
            calldata simulation. Sub-100ms SSE from indexer to your
            cockpit. Open source, MIT.
          </p>

          <div className="mt-10 flex flex-wrap items-center gap-3">
            <Button href="/onboard" variant="primary" size="lg">
              Spin up a solver
            </Button>
            <Button href="/portal" variant="secondary" size="lg">
              Open the portal
            </Button>
          </div>

          {/* Single honest fact line — no ticking counters, no fake numbers */}
          <div className="mt-14 flex flex-wrap items-baseline gap-x-6 gap-y-2 font-mono text-[11px] tracking-[0.18em] uppercase text-[var(--text-tertiary)]">
            <span><span className="text-[var(--text-primary)]">31</span> protocols</span>
            <span aria-hidden>·</span>
            <span><span className="text-[var(--text-primary)]">38+</span> chains</span>
            <span aria-hidden>·</span>
            <span><span className="text-[var(--text-primary)]">SVM + EVM</span></span>
            <span aria-hidden>·</span>
            <span><span className="text-[var(--text-primary)]">MIT</span></span>
            <span aria-hidden>·</span>
            <span><span className="text-[var(--text-primary)]">Rust 1.83</span></span>
          </div>
        </div>

        {/* Right — the runtime, visibly working. One focal motion. */}
        <div className="lg:pl-4">
          <CrossChainVolume />
        </div>
      </div>
    </section>
  )
}

// ── Phases — four phases that all describe what the runtime does ────────
const PHASES = [
  {
    n: 1,
    step: 'Detect',
    title: 'Indexed at the node-runner level',
    body: 'Every intent, order, and book delta from 31 protocols across 38+ chains, captured before it reaches any public feed. Server-sent events deliver raw signal in under 100 ms.',
    stat: 'latency',
    stat_value: '<100ms',
    tone: 'blue' as const,
  },
  {
    n: 2,
    step: 'Prove',
    title: 'Profitability gate, twelve stages',
    body: 'Gas-aware, slippage-aware, MEV-aware. Every intent runs through the gate before a byte of calldata is built. Skipped, dry-run, executed — every outcome is logged.',
    stat: 'stages',
    stat_value: '12',
    tone: 'blue' as const,
  },
  {
    n: 3,
    step: 'Settle',
    title: 'Solana and EVM, same runtime',
    body: 'One Rust binary. Across V3, deBridge DLN, Mayan Swift, Wormhole, LayerZero, CCTP. Anchor IDLs, JITO bundles, priority fees on the SVM side; EIP-1559, calldata simulation, revert-aware retries on the EVM side.',
    stat: 'targets',
    stat_value: 'SVM + EVM',
    tone: 'mint' as const,
  },
  {
    n: 4,
    step: 'Stream',
    title: 'SSE to your cockpit, sub-second',
    body: 'Lambda lifecycle bar, twelve stages per intent. P&L attribution per protocol, per chain, per wallet. Drill into every tx hash, gas cost, profit calculation. No polling.',
    stat: 'transport',
    stat_value: 'SSE',
    tone: 'blue' as const,
  },
]

function Phases() {
  return (
    <section aria-label="The binary — four phases" className="max-w-[1280px] mx-auto px-6 py-32">
      <div className="grid lg:grid-cols-[1fr_2fr] gap-12">
        <div>
          <Tag>The binary</Tag>
          <h2 className="tf-display tf-gradient-silver mt-6 text-[clamp(2rem,4vw,3.5rem)]">
            Four phases.
            <br />
            One process.
          </h2>
          <p className="mt-6 text-[var(--text-secondary)] leading-relaxed max-w-[480px]">
            The same indexer, proof system, and signal layer that powers
            taifoon.io — packaged as a runtime you can spin up in minutes.
          </p>
        </div>

        <div className="relative">
          <div className="absolute left-[5px] top-2 bottom-2 w-px bg-[var(--border-default)]" />
          <ol className="space-y-16">
            {PHASES.map((p) => (
              <li key={p.n} className="relative pl-10">
                <div
                  className="absolute left-0 top-2 w-2.5 h-2.5 rounded-[1px]"
                  style={{
                    background: p.tone === 'mint' ? 'var(--solana-mint)' : 'var(--brand-blue)',
                  }}
                />
                <PhaseLabel phase={p.n} step={p.step} tone={p.tone} />
                <h3 className="mt-3 text-[var(--text-primary)] font-light tracking-tight text-[clamp(1.5rem,2.5vw,2rem)]">
                  {p.title}
                </h3>
                <p className="mt-4 text-[var(--text-secondary)] leading-[1.7] max-w-[640px]">
                  {p.body}
                </p>
                <div className="mt-5 inline-flex items-baseline gap-3">
                  <span className="tf-stat-prefix">{p.stat}</span>
                  <span
                    className="font-mono text-[20px]"
                    style={{
                      color: p.tone === 'mint' ? 'var(--solana-mint)' : 'var(--brand-blue)',
                    }}
                  >
                    {p.stat_value}
                  </span>
                </div>
              </li>
            ))}
          </ol>
        </div>
      </div>
    </section>
  )
}

// ── Live data + portal preview ───────────────────────────────────────────
function LiveData() {
  return (
    <section aria-label="The portal" className="border-y border-[var(--border-subtle)]">
      <div className="max-w-[1280px] mx-auto px-6 py-32">
        <div className="grid lg:grid-cols-[1fr_1.1fr] gap-16 items-start">
          <div>
            <Tag>The portal</Tag>
            <h2 className="tf-display tf-gradient-silver mt-6 text-[clamp(2rem,4vw,3.25rem)]">
              Every fill,
              <br />
              every stage.
            </h2>
            <p className="mt-6 text-[var(--text-secondary)] leading-relaxed">
              The portal is the operator&apos;s cockpit. Every stage of the
              lambda lifecycle, every dry-run, every revert, every fee
              paid — broadcast over SSE at sub-second latency.
            </p>

            {/* 31 protocols as a calm 4-column grid (the marquee was
                kinetic begging). Static. Sortable by eye. */}
            <div className="mt-10">
              <div className="font-mono text-[10px] tracking-[0.24em] uppercase text-[var(--text-tertiary)] mb-3">
                Indexed protocols
              </div>
              <div className="grid grid-cols-2 sm:grid-cols-3 gap-x-4 gap-y-1.5">
                {PROTOCOLS.map((p) => {
                  const key = p.toLowerCase().split(' ')[0]
                  const color = protocolColors[key] ?? '#94B0C4'
                  return (
                    <div
                      key={p}
                      className="flex items-center gap-2 font-mono text-[11px] text-[var(--text-secondary)]"
                    >
                      <span
                        className="w-1 h-1 shrink-0"
                        style={{ background: color }}
                      />
                      <span className="truncate">{p}</span>
                    </div>
                  )
                })}
              </div>
            </div>

            <div className="mt-10 flex gap-3">
              <Button href="/portal" variant="primary">
                Open the portal
              </Button>
              <Button href="/portal/demo" variant="ghost">
                Live demo
              </Button>
            </div>
          </div>

          <div className="relative">
            <Card padding="none" className="overflow-hidden">
              <LiveTicker />
            </Card>
          </div>
        </div>
      </div>
    </section>
  )
}

// ── Install — one snippet, no headline restating the homepage ────────────
function Install() {
  return (
    <section aria-label="Install" className="max-w-[860px] mx-auto px-6 py-32">
      <Tag>Get on the grid</Tag>
      <h2 className="tf-display tf-gradient-silver mt-6 text-[clamp(2rem,4vw,3rem)]">
        Three commands.
      </h2>
      <p className="mt-6 text-[var(--text-secondary)] max-w-[560px] leading-relaxed">
        Open source, MIT. Fork the runtime, fork the indexer, run on
        testnet by lunch.
      </p>

      <div className="mt-12">
        <Snippet
          variant="tabbed"
          tabs={[
            {
              label: 'install',
              code: 'curl -fsSL solver.taifoon.dev/install.sh | sh',
            },
            {
              label: 'onboard',
              code: 'taifoon-cli onboard --chains base,solana --protocols across,debridge,mayan',
            },
            {
              label: 'run',
              code: 'taifoon-cli run --stream prod',
            },
          ]}
        />
      </div>

      <div className="mt-10 flex gap-3 flex-wrap">
        <Button href="/onboard" variant="primary" size="lg">
          Spin up a solver
        </Button>
        <Button href="/portal" variant="secondary" size="lg">
          Open the portal
        </Button>
      </div>
    </section>
  )
}
