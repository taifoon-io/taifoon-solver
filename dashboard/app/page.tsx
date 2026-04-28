'use client'

import { NavBar, Footer, Card, Button, CodeBlock, StatTile, Tag, PhaseLabel, Badge } from '@/components/ui'
import { protocolColors } from '@/lib/tokens'

const PROTOCOLS = [
  'Across V3', 'deBridge DLN', 'Mayan Swift', 'LiFi', 'Stargate V2', 'CCTP',
  'Hop', 'Connext', 'Synapse', 'Celer cBridge', 'Wormhole', 'LayerZero v2',
  'Hyperlane', 'Axelar GMP', 'CCIP', 'Socket', 'Squid Router', 'Rango',
  'Symbiosis', 'Meson', 'Allbridge', 'Router Protocol', 'Orbiter',
  't3rn LWC', 'Relay', 'Arbitrum native', 'Optimism native',
  '1inch Fusion', 'Uniswap v4', 'Bancor', 'Wormhole NTT',
] as const

const CHAINS = [
  { id: 1, label: 'Ethereum' },
  { id: 8453, label: 'Base' },
  { id: 42161, label: 'Arbitrum' },
  { id: 10, label: 'Optimism' },
  { id: 137, label: 'Polygon' },
  { id: 56, label: 'BSC' },
  { id: 43114, label: 'Avalanche' },
  { id: 900, label: 'Solana', solana: true },
] as const

export default function LandingPage() {
  return (
    <>
      <NavBar />
      <main className="flex-1">
        <Hero />
        <ProtocolMarquee />
        <Phases />
        <Flywheel />
        <DashboardPreview />
        <ColosseumStrip />
        <FinalCTA />
      </main>
      <Footer />
    </>
  )
}

// ── Hero ───────────────────────────────────────────────────────────────────
function Hero() {
  return (
    <section className="relative overflow-hidden">
      <div className="absolute inset-0 dot-grid pointer-events-none opacity-60" />
      <div className="absolute inset-0 glow-bg pointer-events-none" />

      <div className="relative max-w-[1280px] mx-auto px-6 pt-28 pb-24">
        <div className="text-center mb-10">
          <span className="inline-flex items-center gap-3 font-mono text-[11px] tracking-[0.25em] text-[var(--text-secondary)]">
            <span className="w-1.5 h-1.5 rounded-full bg-[var(--solana-mint)] pulse-live" />
            TAIFOON · SPINNERS · 31 PROTOCOLS · SOLANA + EVM
          </span>
        </div>

        <h1
          className="tf-display tf-gradient-silver text-center mx-auto max-w-[1100px]"
          style={{ fontSize: 'clamp(2.75rem, 8vw, 6rem)' }}
        >
          One spinner.
          <br />
          Every protocol.
          <br />
          Every chain.
        </h1>

        <p className="mt-10 text-center text-[var(--text-secondary)] max-w-[640px] mx-auto leading-[1.7] text-[16px]">
          The autonomous spinner runtime — Taifoon&apos;s solver core,
          unbundled and packaged for hackathon teams. 31 protocols, 38+
          chains, Solana and EVM under one cryptographic root. Spin up,
          watch fills land, keep the spread.
        </p>

        <div className="mt-12 flex flex-wrap items-center justify-center gap-3">
          <Button href="/onboard" variant="primary" size="lg">
            <span className="text-[var(--text-tertiary)]">{'>'}</span>
            SPIN UP A SPINNER
            <span>→</span>
          </Button>
          <Button href="/portal" variant="secondary" size="lg">
            OPEN THE PORTAL
          </Button>
        </div>

        {/* Stat callouts — taifoon.io style: tiny prefix, mono numbers */}
        <div className="mt-20 grid grid-cols-2 sm:grid-cols-4 gap-x-12 gap-y-6 max-w-[920px] mx-auto">
          <StatTile label="PROTOCOLS" value="31" tone="default" />
          <StatTile label="CHAINS" value="38+" tone="default" />
          <StatTile label="SVM CHAINS" value="2" tone="mint" />
          <StatTile label="MEDIAN LATENCY" value="127" unit="ms" tone="blue" />
        </div>
      </div>
    </section>
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

// ── Phases (vertical timeline, taifoon.io style) ─────────────────────────
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

        {/* Vertical timeline */}
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

// ── Flywheel — Solana + EVM dual section (taifoon.io flywheel echo) ──────
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

// ── Dashboard preview ──────────────────────────────────────────────────────
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

        {/* Decorative preview */}
        <div className="relative">
          <Card padding="none" className="overflow-hidden">
            <div className="flex items-center justify-between px-4 py-3 border-b border-[var(--border-subtle)] bg-[var(--bg-elevated)]">
              <div className="flex items-center gap-3">
                <span className="w-1.5 h-1.5 rounded-full bg-[var(--solana-mint)] pulse-live" />
                <span className="font-mono text-[11px] tracking-[0.16em] text-[var(--text-secondary)]">
                  spinner_b3e9a2
                </span>
                <Badge tone="info">5 PROTOCOLS</Badge>
              </div>
              <span className="font-mono text-[11px] text-[var(--solana-mint)] tracking-[0.12em]">
                + $12.43
              </span>
            </div>
            <div className="p-4 space-y-2 bg-black">
              <FakeRow proto="Across V3" src="ETH" dst="ARB" amt="10,000 USDC" profit={43.9} stage="confirmed" />
              <FakeRow proto="deBridge DLN" src="BASE" dst="SOL" amt="2,500 USDC" profit={18.2} stage="broadcast" />
              <FakeRow proto="Mayan Swift" src="SOL" dst="BASE" amt="800 SOL" profit={6.7} stage="calldata_build" />
              <FakeRow proto="LiFi" src="ARB" dst="OP" amt="50K USDC" profit={0.83} stage="dry_run" />
              <FakeRow proto="Stargate V2" src="OP" dst="BSC" amt="20K USDT" profit={-0.12} stage="skipped" />
            </div>
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

function FakeRow({
  proto,
  src,
  dst,
  amt,
  profit,
  stage,
}: {
  proto: string
  src: string
  dst: string
  amt: string
  profit: number
  stage: 'confirmed' | 'broadcast' | 'calldata_build' | 'dry_run' | 'skipped'
}) {
  const key = proto.toLowerCase().split(' ')[0]
  const color = protocolColors[key] ?? '#94B0C4'
  const stageMap = {
    confirmed: { label: 'CONFIRMED', tone: '#14F195' },
    broadcast: { label: 'BROADCAST', tone: '#3DA5FF' },
    calldata_build: { label: 'CALLDATA', tone: '#9945FF' },
    dry_run: { label: 'DRY-RUN', tone: '#FFB454' },
    skipped: { label: 'SKIPPED', tone: '#94B0C4' },
  }
  const s = stageMap[stage]
  return (
    <div className="flex items-center justify-between gap-2 px-3 py-2.5 border-l-2"
         style={{ borderLeftColor: color, background: `${color}05` }}>
      <div className="flex items-center gap-3 min-w-0">
        <span
          className="text-[10px] font-mono tracking-[0.12em] uppercase"
          style={{ color }}
        >
          {proto}
        </span>
        <span className="font-mono text-[10px] text-[var(--text-tertiary)]">
          {src} → {dst}
        </span>
        <span className="font-mono text-[10px] text-[var(--text-secondary)] truncate">
          {amt}
        </span>
      </div>
      <div className="flex items-center gap-3 shrink-0">
        <span
          className={`font-mono text-[11px] ${
            profit >= 0 ? 'text-[var(--solana-mint)]' : 'text-[var(--text-tertiary)]'
          }`}
        >
          {profit >= 0 ? '+' : ''}${profit.toFixed(2)}
        </span>
        <span
          className="text-[9px] font-mono tracking-[0.18em] px-1.5 py-0.5 rounded-[2px]"
          style={{ color: s.tone, border: `1px solid ${s.tone}40` }}
        >
          {s.label}
        </span>
      </div>
    </div>
  )
}

// ── Colosseum strip ──────────────────────────────────────────────────────
function ColosseumStrip() {
  return (
    <section className="border-y border-[var(--border-subtle)]">
      <div className="max-w-[1280px] mx-auto px-6 py-24">
        <div className="grid md:grid-cols-[1.3fr_1fr] gap-16 items-center">
          <div>
            <Tag tone="mint">Solana colosseum</Tag>
            <h2 className="tf-display tf-gradient-solana mt-6 text-[clamp(2rem,4vw,3rem)]">
              Built so you can ship
              <br />
              a winning bridge spinner
              <br />
              this weekend.
            </h2>
            <p className="mt-6 text-[var(--text-secondary)] leading-relaxed max-w-[560px]">
              We&apos;ve already done the parts that take weeks: the
              indexer, the genome stream, the protocol adapters, the
              lifecycle state machine. You bring the edge — a routing
              trick, a gas heuristic, a custom intent type — the runtime
              takes care of the rest.
            </p>
            <div className="mt-8 flex flex-wrap gap-3">
              <Button href="/onboard" variant="mint">
                START THE WIZARD →
              </Button>
              <Button
                href="https://github.com/yawningmonsoon/taifoon-solver"
                external
                variant="secondary"
              >
                READ THE SOURCE
              </Button>
            </div>
          </div>

          <div className="space-y-px bg-[var(--border-subtle)]">
            {[
              { label: 'HOSTED INDEXER', value: 'Genome SSE — 31 protocols, $0 for hackathon teams' },
              { label: 'ON-CHAIN REGISTRATION', value: 'Auto-deploy to Base Sepolia + Solana Devnet on signup' },
              { label: 'ONE POD PER SPINNER', value: 'K8s manifests included — fork or self-host' },
              { label: 'CHAINS SUPPORTED', value: CHAINS.map(c => c.label).join(' · ') },
            ].map((row) => (
              <div key={row.label} className="bg-[var(--bg-base)] px-5 py-4">
                <div className="tf-stat-prefix uppercase tracking-[0.24em] mb-1.5">
                  {row.label}
                </div>
                <div className="text-sm text-[var(--text-primary)]">{row.value}</div>
              </div>
            ))}
          </div>
        </div>
      </div>
    </section>
  )
}

// ── Final CTA ──────────────────────────────────────────────────────────────
function FinalCTA() {
  return (
    <section className="max-w-[1100px] mx-auto px-6 py-32 text-center">
      <Tag>Get on the grid</Tag>
      <h2 className="tf-display tf-gradient-silver mt-6 text-[clamp(2.5rem,6vw,4.5rem)]">
        Your spinner is
        <br />
        one command away.
      </h2>
      <div className="mt-12 max-w-[560px] mx-auto">
        <CodeBlock
          lang="bash"
          code={`curl -fsSL solver.taifoon.dev/install.sh | sh
taifoon-cli onboard --chains base,solana --protocols across,debridge,mayan
taifoon-cli run`}
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
