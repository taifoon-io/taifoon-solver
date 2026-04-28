'use client'

import Link from 'next/link'
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
import { Counter } from '@/components/marketing/Counter'
import { protocolColors } from '@/lib/tokens'

const PROTOCOLS = [
  'Across V3', 'deBridge DLN', 'Mayan Swift', 'LiFi', 'Stargate V2', 'CCTP',
  'Hop', 'Connext', 'Synapse', 'Celer cBridge', 'Wormhole', 'LayerZero v2',
  'Hyperlane', 'Axelar GMP', 'CCIP', 'Socket', 'Squid Router', 'Rango',
  'Symbiosis', 'Meson', 'Allbridge', 'Router Protocol', 'Orbiter',
  't3rn LWC', 'Relay', 'Arbitrum native', 'Optimism native',
  '1inch Fusion', 'Uniswap v4', 'Bancor', 'Wormhole NTT',
] as const

export default function LandingPage() {
  return (
    <>
      <NewsBand />
      <NavBar />
      <main className="flex-1">
        <Hero />
        <ProtocolMarquee />
        <Phases />
        <Flywheel />
        <DashboardPreview />
        <FinalCTA />
      </main>
      <Footer />
    </>
  )
}

// ── Cinematic hero ────────────────────────────────────────────────────────
function Hero() {
  return (
    <section className="relative overflow-hidden min-h-[760px] flex items-center">
      {/* layers, back to front: orbits → grid → glow → content */}
      <ChainOrbits className="opacity-50" />
      <div className="absolute inset-0 dot-grid pointer-events-none opacity-30" />
      <div className="absolute inset-0 glow-bg pointer-events-none" />
      {/* vignette so type stays legible over the orbits */}
      <div
        className="absolute inset-0 pointer-events-none"
        style={{
          background:
            'radial-gradient(ellipse at center, transparent 30%, rgba(0,0,0,0.85) 75%)',
        }}
      />

      <div className="relative w-full max-w-[1280px] mx-auto px-6 py-24 grid lg:grid-cols-[1.15fr_1fr] gap-12 items-center">
        {/* Left — copy */}
        <div>
          <div className="inline-flex items-center gap-3 font-mono text-[11px] tracking-[0.25em] text-[var(--text-secondary)] mb-8">
            <span className="w-1.5 h-1.5 rounded-full bg-[var(--solana-mint)] pulse-live" />
            TAIFOON · SPINNERS · SOLANA + EVM
          </div>

          <h1
            className="tf-display tf-gradient-silver"
            style={{ fontSize: 'clamp(3rem, 7.5vw, 6.25rem)' }}
          >
            One spinner.
            <br />
            <span className="tf-gradient-solana">Every protocol.</span>
            <br />
            Every chain.
          </h1>

          <p className="mt-8 text-[var(--text-secondary)] max-w-[540px] leading-[1.7] text-[16px]">
            The autonomous bridge-solver runtime, unbundled from
            taifoon.io and packaged for hackathon teams. 31 protocols,
            38+ chains, Solana and EVM under one cryptographic root.
            Spin up, watch fills land, keep the spread.
          </p>

          <div className="mt-10 flex flex-wrap items-center gap-3">
            <Button href="/onboard" variant="primary" size="lg">
              <span className="text-[var(--text-tertiary)]">{'>'}</span>
              SPIN UP A SPINNER →
            </Button>
            <Button href="/portal" variant="secondary" size="lg">
              OPEN THE PORTAL
            </Button>
          </div>

          {/* Live KPI strip — these tick up so the hero feels alive */}
          <div className="mt-14 grid grid-cols-2 sm:grid-cols-4 gap-x-10 gap-y-6">
            <KPI label="$ FILLED · 24H" value={
              <Counter base={28430} step={4.2} format="usd" prefix="$" />
            } tone="mint" />
            <KPI label="FILLS · 24H" value={
              <Counter base={2841} step={1} format="int" />
            } tone="blue" />
            <KPI label="PROTOCOLS" value="31" tone="default" />
            <KPI label="CHAINS" value="38+" tone="default" />
          </div>
        </div>

        {/* Right — the runtime, visibly working */}
        <div className="lg:pl-4">
          <LiveTicker />

          <div className="mt-4 flex items-center justify-between font-mono text-[10px] tracking-[0.2em] uppercase text-[var(--text-tertiary)]">
            <span>Proof-of-runtime · not a mockup</span>
            <PortalLink />
          </div>
        </div>
      </div>
    </section>
  )
}

function PortalLink() {
  return (
    <Link href="/portal" className="hover:text-[var(--brand-blue)] transition-colors">
      SEE LIVE PORTAL →
    </Link>
  )
}

function KPI({
  label,
  value,
  tone,
}: {
  label: string
  value: React.ReactNode
  tone: 'mint' | 'blue' | 'default'
}) {
  const c = {
    mint: 'text-[var(--solana-mint)]',
    blue: 'text-[var(--brand-blue)]',
    default: 'text-[var(--text-primary)]',
  }
  return (
    <div className="flex flex-col gap-1.5">
      <span className="font-mono text-[10px] tracking-[0.24em] uppercase text-[var(--text-tertiary)]">
        {label}
      </span>
      <span className={`font-mono text-[26px] tracking-[-0.01em] ${c[tone]}`}>
        {value}
      </span>
    </div>
  )
}

// ── Protocol marquee ──────────────────────────────────────────────────────
function ProtocolMarquee() {
  const items = [...PROTOCOLS, ...PROTOCOLS]
  return (
    <section className="border-y border-[var(--border-subtle)]">
      <div className="max-w-[1400px] mx-auto px-6 py-8">
        <div className="tf-tag text-center mb-5">[ INDEXED PROTOCOLS ]</div>
        <div
          className="overflow-hidden"
          style={{
            maskImage:
              'linear-gradient(to right, transparent, black 8%, black 92%, transparent)',
            WebkitMaskImage:
              'linear-gradient(to right, transparent, black 8%, black 92%, transparent)',
          }}
        >
          <div className="marquee-track flex gap-3">
            {items.map((p, i) => {
              const key = p.toLowerCase().split(' ')[0]
              const color = protocolColors[key] ?? '#94B0C4'
              return (
                <span
                  key={i}
                  className="inline-flex items-center gap-2 px-3 h-8 rounded-[2px] border whitespace-nowrap"
                  style={{
                    borderColor: 'var(--border-default)',
                    color: 'var(--text-secondary)',
                  }}
                >
                  <span className="w-1 h-1" style={{ background: color }} />
                  <span className="text-[11px] font-mono tracking-[0.12em]">{p}</span>
                </span>
              )
            })}
          </div>
        </div>
      </div>
    </section>
  )
}

// ── Phases ────────────────────────────────────────────────────────────────
const PHASES = [
  {
    n: 1,
    step: 'Detect',
    title: 'The market sees it after we do',
    body: '31 protocols, 38+ chains. Every intent, every order, every order-book delta — collected at the node-runner level before it reaches any public feed. SSE delivers raw signal to your spinner in under 100 ms.',
    stat: 'real-time',
    stat_value: '31 protocols',
    tone: 'blue' as const,
  },
  {
    n: 2,
    step: 'Prove',
    title: 'Profitability, not promises',
    body: 'Every intent runs through a gas-aware, slippage-aware, MEV-aware profitability gate before a single byte of calldata is built. Skipped, dry-run, executed — every outcome is logged and visible.',
    stat: 'gate',
    stat_value: '12 stages',
    tone: 'blue' as const,
  },
  {
    n: 3,
    step: 'Settle',
    title: 'Solana and EVM, same runtime',
    body: 'A single Rust binary that speaks Across V3, deBridge DLN, Mayan Swift, Wormhole, LayerZero, CCTP — Anchor IDLs, JITO bundles, priority fees on the SVM side; EIP-1559, calldata simulation, revert-aware retries on the EVM side.',
    stat: 'fluent',
    stat_value: 'SVM + EVM',
    tone: 'mint' as const,
  },
  {
    n: 4,
    step: 'Compound',
    title: 'Every fill funds the next one',
    body: 'Spinners earn protocol fees. Fees fund more wallets. More wallets fill more intents. The flywheel is autonomous. The runtime is open. The edge belongs to the operator.',
    stat: 'flywheel',
    stat_value: '∞',
    tone: 'mint' as const,
  },
]

function Phases() {
  return (
    <section className="max-w-[1280px] mx-auto px-6 py-32">
      <div className="grid lg:grid-cols-[1fr_2fr] gap-12">
        <div>
          <Tag>The engine</Tag>
          <h2 className="tf-display tf-gradient-silver mt-6 text-[clamp(2rem,4vw,3.5rem)]">
            One grid.
            <br />
            Every spinner.
          </h2>
          <p className="mt-6 text-[var(--text-secondary)] leading-relaxed max-w-[480px]">
            The same indexer, proof system, and signal layer that powers
            taifoon.io — exposed as a runtime you can spin up in minutes.
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

// ── Flywheel ──────────────────────────────────────────────────────────────
function Flywheel() {
  return (
    <section className="border-y border-[var(--border-subtle)]">
      <div className="max-w-[1280px] mx-auto px-6 py-32">
        <div className="text-center mb-16">
          <Tag>The economy</Tag>
          <h2 className="tf-display tf-gradient-silver mt-6 text-[clamp(2.25rem,4vw,3.5rem)]">
            Every participant
            <br />
            feeds the same flywheel.
          </h2>
          <p className="mt-6 text-[var(--text-secondary)] leading-relaxed max-w-[640px] mx-auto">
            Taifoon&apos;s grid pays the people who make it stronger. Spinners
            earn execution fees from the autonomous DeFi economy — the more
            you fill, the more the protocol funds the next spinner.
          </p>
        </div>

        <div className="grid md:grid-cols-3 gap-px bg-[var(--border-subtle)]">
          {[
            {
              tag: 'Spinners',
              tone: 'blue' as const,
              contributes:
                'GPU-aware solver pods that execute intents end-to-end. One process, many wallets.',
              receives:
                'Execution fees from every successful fill. Auto-scaled by the protocol when demand spikes.',
            },
            {
              tag: 'Protocols',
              tone: 'mint' as const,
              contributes:
                '31 cross-chain bridges, intent systems, and aggregators — pre-integrated in the adapter layer.',
              receives:
                'Tighter spreads, faster fills, more reliable settlement on every supported chain.',
            },
            {
              tag: 'Operators',
              tone: 'violet' as const,
              contributes:
                'A wallet, a chain selection, a profitability heuristic. The smallest possible footprint.',
              receives:
                'A live portal, on-chain attribution, and a slice of the autonomous DeFi economy.',
            },
          ].map((row) => (
            <div key={row.tag} className="bg-[var(--bg-base)] p-8">
              <Tag tone={row.tone}>{row.tag}</Tag>
              <div className="mt-6">
                <div className="tf-stat-prefix uppercase tracking-[0.24em] mb-2">
                  contributes
                </div>
                <p className="text-sm text-[var(--text-primary)] leading-relaxed">
                  {row.contributes}
                </p>
              </div>
              <div className="mt-6">
                <div className="tf-stat-prefix uppercase tracking-[0.24em] mb-2">
                  receives
                </div>
                <p className="text-sm text-[var(--text-secondary)] leading-relaxed">
                  {row.receives}
                </p>
              </div>
            </div>
          ))}
        </div>
      </div>
    </section>
  )
}

// ── Dashboard preview ─────────────────────────────────────────────────────
function DashboardPreview() {
  return (
    <section className="max-w-[1280px] mx-auto px-6 py-32">
      <div className="grid lg:grid-cols-[1fr_1.1fr] gap-16 items-center">
        <div>
          <Tag>The cockpit</Tag>
          <h2 className="tf-display tf-gradient-silver mt-6 text-[clamp(2rem,4vw,3.25rem)]">
            A glass box
            <br />
            for every fill.
          </h2>
          <p className="mt-6 text-[var(--text-secondary)] leading-relaxed">
            The portal isn&apos;t a vanity dashboard — it&apos;s the operator&apos;s
            cockpit. Every stage of the lambda lifecycle, every dry-run,
            every revert, every fee paid, broadcast over server-sent
            events at sub-second latency.
          </p>
          <ul className="mt-8 space-y-3 text-sm text-[var(--text-secondary)]">
            <Bullet>Lambda lifecycle bar — twelve stages, one row per intent.</Bullet>
            <Bullet>P&amp;L attribution per protocol, per chain, per wallet.</Bullet>
            <Bullet>SSE stream from the Rust core — no polling, no lag.</Bullet>
            <Bullet>Drill into every tx hash, gas cost, profit calc.</Bullet>
          </ul>
          <div className="mt-10 flex gap-3">
            <Button href="/portal" variant="primary">
              OPEN THE PORTAL →
            </Button>
            <Button href="/portal/demo" variant="ghost">
              LIVE DEMO
            </Button>
          </div>
        </div>

        <div className="relative">
          <Card padding="none" className="overflow-hidden">
            <LiveTicker />
          </Card>
        </div>
      </div>
    </section>
  )
}

function Bullet({ children }: { children: React.ReactNode }) {
  return (
    <li className="flex gap-3">
      <span className="mt-2 w-1.5 h-1.5 rounded-[1px] bg-[var(--brand-blue)] shrink-0" />
      <span className="leading-relaxed">{children}</span>
    </li>
  )
}

// ── Final CTA — folded Colosseum framing into copy + tabbed snippet ──────
function FinalCTA() {
  return (
    <section className="max-w-[1100px] mx-auto px-6 py-32 text-center">
      <Tag>Get on the grid</Tag>
      <h2 className="tf-display tf-gradient-silver mt-6 text-[clamp(2.5rem,6vw,4.5rem)]">
        Your spinner is
        <br />
        one command away.
      </h2>
      <p className="mt-6 text-[var(--text-secondary)] max-w-[560px] mx-auto leading-relaxed">
        Open-source, MIT, free for hackathon teams — Solana Colosseum
        included. Fork the runtime, fork the indexer, run your spinner
        on testnet by lunch.
      </p>

      <div className="mt-12 max-w-[640px] mx-auto text-left">
        <Snippet
          variant="tabbed"
          tabs={[
            {
              label: 'INSTALL',
              code: 'curl -fsSL solver.taifoon.dev/install.sh | sh',
            },
            {
              label: 'ONBOARD',
              code: 'taifoon-cli onboard --chains base,solana --protocols across,debridge,mayan',
            },
            {
              label: 'RUN',
              code: 'taifoon-cli run --stream prod',
            },
          ]}
        />
      </div>

      <div className="mt-10 flex justify-center gap-3 flex-wrap">
        <Button href="/onboard" variant="primary" size="lg">
          SPIN UP A SPINNER →
        </Button>
        <Button href="/portal" variant="secondary" size="lg">
          OPEN THE PORTAL
        </Button>
      </div>
    </section>
  )
}
