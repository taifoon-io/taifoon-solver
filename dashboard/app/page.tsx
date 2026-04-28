'use client'

import { NavBar, Footer, Card, CardHeader, Badge, Button, CodeBlock, StatTile } from '@/components/ui'
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
        <SolanaSection />
        <HowItWorks />
        <DashboardPreview />
        <ColosseumSection />
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
      <div className="absolute inset-0 mesh-bg pointer-events-none" />
      <div className="absolute inset-0 grid-bg opacity-40 pointer-events-none" />
      <div className="relative max-w-[1200px] mx-auto px-6 pt-20 pb-24">
        <div className="flex justify-center mb-6">
          <Badge tone="violet" dot pulse>
            Solana Colosseum × cross-chain · v0.1 live
          </Badge>
        </div>

        <h1
          className="text-center font-bold tracking-[-0.03em] leading-[0.95]"
          style={{ fontSize: 'clamp(2.5rem, 7vw, 5rem)' }}
        >
          The fastest way to run<br />
          <span className="text-gradient-brand">a profitable cross-chain solver.</span>
        </h1>

        <p className="mt-7 text-center text-[var(--text-secondary)] max-w-[640px] mx-auto leading-relaxed text-lg">
          Open-source solver runtime for{' '}
          <span className="text-[var(--brand-cyan)] font-semibold">31 protocols</span> across{' '}
          <span className="text-[var(--text-primary)] font-semibold">38+ chains</span> — including{' '}
          <span className="text-[var(--brand-violet)] font-semibold">Solana</span>. Spin up,
          watch fills land, keep the spread.
        </p>

        <div className="mt-10 flex flex-wrap items-center justify-center gap-3">
          <Button href="/onboard" variant="glow" size="lg">
            Spin up your solver →
          </Button>
          <Button href="/portal" variant="secondary" size="lg">
            Live portal
          </Button>
          <Button href="https://github.com/yawningmonsoon/taifoon-solver" external variant="ghost" size="lg">
            View on GitHub ↗
          </Button>
        </div>

        <div className="mt-14 grid grid-cols-2 sm:grid-cols-4 gap-3 max-w-[860px] mx-auto">
          <StatTile label="Protocols supported" value="31" tone="cyan" />
          <StatTile label="Chains covered" value="38+" tone="violet" />
          <StatTile label="Lambda stages tracked" value="12" />
          <StatTile label="Median fill latency" value="127" unit="ms" tone="success" />
        </div>
      </div>
    </section>
  )
}

// ── Protocol marquee ──────────────────────────────────────────────────────
function ProtocolMarquee() {
  const items = [...PROTOCOLS, ...PROTOCOLS]
  return (
    <section className="border-y border-[var(--border-subtle)] bg-[var(--bg-elevated)]/50">
      <div className="max-w-[1400px] mx-auto px-6 py-7">
        <div className="text-[10px] text-[var(--text-tertiary)] uppercase tracking-[0.2em] text-center mb-4">
          Solving for every protocol Taifoon supports
        </div>
        <div
          className="overflow-hidden relative"
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
              const color = protocolColors[key] ?? '#A0A0B0'
              return (
                <span
                  key={i}
                  className="inline-flex items-center gap-2 px-4 h-9 rounded-[var(--r-pill)] border whitespace-nowrap"
                  style={{
                    borderColor: `${color}33`,
                    background: `${color}0a`,
                    color: color,
                  }}
                >
                  <span className="w-1.5 h-1.5 rounded-full" style={{ background: color }} />
                  <span className="text-[12px] font-medium">{p}</span>
                </span>
              )
            })}
          </div>
        </div>
      </div>
    </section>
  )
}

// ── Solana × EVM ──────────────────────────────────────────────────────────
function SolanaSection() {
  return (
    <section className="max-w-[1200px] mx-auto px-6 py-24">
      <div className="grid lg:grid-cols-2 gap-10 items-center">
        <div>
          <Badge tone="violet">Solana-native solving</Badge>
          <h2 className="mt-4 text-4xl md:text-5xl font-bold tracking-tight leading-tight">
            One runtime. <span className="text-gradient-solana">Solana and EVM.</span>
          </h2>
          <p className="mt-5 text-[var(--text-secondary)] leading-relaxed">
            Solving Solana in isolation is hard. Solving cross-chain across Solana,
            Base, Arbitrum, Ethereum, and 6+ more is harder. Taifoon ships a single
            Rust solver that speaks deBridge DLN, Mayan Swift, Wormhole, LayerZero,
            and CCTP — fluently, on day one.
          </p>
          <ul className="mt-6 space-y-3 text-sm text-[var(--text-secondary)]">
            <Bullet>Native Solana adapter — Anchor IDLs, JITO bundles, priority fees.</Bullet>
            <Bullet>EVM coverage — Across V3, deBridge, Stargate, LiFi, all from one binary.</Bullet>
            <Bullet>Lambda lifecycle telemetry — every stage from detection to confirmation.</Bullet>
            <Bullet>Profitability gates — gas-aware, slippage-aware, MEV-aware.</Bullet>
          </ul>
        </div>

        <Card padding="md" className="bg-[var(--bg-elevated)]">
          <CardHeader
            title="Supported chains"
            subtitle="More added each week — operators can also self-register chains."
          />
          <div className="grid grid-cols-2 gap-2">
            {CHAINS.map((c) => (
              <div
                key={c.id}
                className={`flex items-center justify-between px-3 py-2.5 rounded-[var(--r-md)] border ${
                  'solana' in c && c.solana
                    ? 'border-[var(--brand-violet)]/40 bg-[var(--brand-violet)]/5'
                    : 'border-[var(--border-default)] bg-[var(--bg-raised)]'
                }`}
              >
                <span
                  className={`text-sm font-medium ${
                    'solana' in c && c.solana
                      ? 'text-[var(--brand-violet)]'
                      : 'text-[var(--text-primary)]'
                  }`}
                >
                  {c.label}
                </span>
                <span className="font-mono text-[10px] text-[var(--text-tertiary)]">
                  #{c.id}
                </span>
              </div>
            ))}
          </div>
          <div className="mt-4 text-[11px] text-[var(--text-tertiary)] flex items-center gap-2">
            <span className="w-2 h-2 rounded-full bg-[var(--success)] pulse-live" />
            Mainnet + testnet in active rotation.
          </div>
        </Card>
      </div>
    </section>
  )
}

function Bullet({ children }: { children: React.ReactNode }) {
  return (
    <li className="flex gap-3">
      <span className="mt-1.5 w-1.5 h-1.5 rounded-full bg-[var(--brand-cyan)] shrink-0" />
      <span>{children}</span>
    </li>
  )
}

// ── How it works ───────────────────────────────────────────────────────────
function HowItWorks() {
  return (
    <section className="max-w-[1200px] mx-auto px-6 py-20">
      <div className="text-center mb-14">
        <Badge tone="info">3 steps</Badge>
        <h2 className="mt-4 text-4xl md:text-5xl font-bold tracking-tight">
          From zero to filling intents.
        </h2>
        <p className="mt-3 text-[var(--text-secondary)] max-w-[600px] mx-auto">
          Designed so a Colosseum hacker can register, deploy, and observe a solver in under
          ten minutes — and a production team can scale the same runtime to a fleet.
        </p>
      </div>

      <div className="grid md:grid-cols-3 gap-4">
        <StepCard
          step={1}
          title="Install the CLI"
          color="cyan"
          description="One Rust binary. Works on Linux, macOS, and inside any k8s pod."
          code={`# install\ncargo install taifoon-cli\n\n# or grab a prebuilt\ncurl -fsSL solver.taifoon.dev/install.sh | sh`}
          lang="bash"
        />
        <StepCard
          step={2}
          title="Register on-chain"
          color="violet"
          description="One command generates a wallet, registers it with the Taifoon Registry on Base + Solana, and writes your config."
          code={`taifoon-cli onboard \\\n  --chains base,solana,arbitrum \\\n  --protocols across,debridge,mayan`}
          lang="bash"
        />
        <StepCard
          step={3}
          title="Run and watch"
          color="glow"
          description="Pod, container, or local — point it at the genome stream and tail your fills in the portal."
          code={`taifoon-cli run --stream prod\n\n# → portal at solver.taifoon.dev/portal/$SOLVER_ID`}
          lang="bash"
        />
      </div>
    </section>
  )
}

function StepCard({
  step,
  title,
  description,
  code,
  lang,
  color,
}: {
  step: number
  title: string
  description: string
  code: string
  lang?: string
  color: 'cyan' | 'violet' | 'glow'
}) {
  const colorMap = {
    cyan: 'var(--brand-cyan)',
    violet: 'var(--brand-violet)',
    glow: 'var(--brand-glow)',
  }
  const c = colorMap[color]
  return (
    <Card padding="md">
      <div
        className="w-9 h-9 rounded-[var(--r-md)] flex items-center justify-center font-mono font-bold mb-4"
        style={{ background: `${c}1a`, color: c, border: `1px solid ${c}44` }}
      >
        0{step}
      </div>
      <div className="text-lg font-bold tracking-tight">{title}</div>
      <p className="text-sm text-[var(--text-secondary)] mt-2 mb-4 leading-relaxed">{description}</p>
      <CodeBlock code={code} lang={lang} />
    </Card>
  )
}

// ── Dashboard preview ──────────────────────────────────────────────────────
function DashboardPreview() {
  return (
    <section className="max-w-[1200px] mx-auto px-6 py-20">
      <div className="grid lg:grid-cols-[1fr_1.1fr] gap-12 items-center">
        <div>
          <Badge tone="success" dot pulse>
            Live by default
          </Badge>
          <h2 className="mt-4 text-4xl md:text-5xl font-bold tracking-tight leading-tight">
            A glass box for every fill.
          </h2>
          <p className="mt-5 text-[var(--text-secondary)] leading-relaxed">
            The portal isn&apos;t a vanity dashboard — it&apos;s the operator&apos;s cockpit.
            Every stage of the Lambda lifecycle, every dry-run, every revert, every
            fee paid — visible in real time, broadcast over server-sent events.
          </p>
          <ul className="mt-6 space-y-3 text-sm text-[var(--text-secondary)]">
            <Bullet>Sub-second SSE updates from the Rust core.</Bullet>
            <Bullet>P&amp;L attribution per protocol, per chain, per intent.</Bullet>
            <Bullet>Lambda lifecycle bar — twelve stages, color-coded.</Bullet>
            <Bullet>Drill-down on every tx hash, gas cost, and profit calc.</Bullet>
          </ul>
          <div className="mt-7 flex gap-3">
            <Button href="/portal" variant="primary">
              Open the portal
            </Button>
            <Button href="/portal/demo" variant="secondary">
              Live demo
            </Button>
          </div>
        </div>

        <div className="relative">
          <div className="absolute -inset-6 bg-gradient-to-br from-[var(--brand-cyan)]/12 to-[var(--brand-violet)]/12 blur-3xl rounded-[40px]" />
          <Card padding="none" className="relative overflow-hidden">
            <div className="flex items-center justify-between px-4 py-3 border-b border-[var(--border-subtle)] bg-[var(--bg-elevated)]">
              <div className="flex items-center gap-2">
                <span className="w-2 h-2 rounded-full bg-[var(--success)] animate-pulse" />
                <span className="font-mono text-xs text-[var(--text-secondary)]">solver_b3e9a2</span>
                <Badge tone="violet">5 protocols</Badge>
              </div>
              <span className="text-xs font-mono text-[var(--success)]">+$12.43</span>
            </div>
            <div className="p-4 space-y-2 bg-[var(--bg-base)]">
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
  const color = protocolColors[key] ?? '#A0A0B0'
  const stageMap = {
    confirmed: { label: 'CONFIRMED', tone: '#00FF88' },
    broadcast: { label: 'BROADCAST', tone: '#00D9FF' },
    calldata_build: { label: 'CALLDATA', tone: '#9945FF' },
    dry_run: { label: 'DRY-RUN', tone: '#FFB800' },
    skipped: { label: 'SKIPPED', tone: '#60606E' },
  }
  const s = stageMap[stage]
  return (
    <div
      className="flex items-center justify-between gap-2 px-3 py-2.5 rounded-[var(--r-md)] border"
      style={{ borderColor: `${color}22`, background: `${color}07` }}
    >
      <div className="flex items-center gap-2 min-w-0">
        <span
          className="text-[10px] font-bold px-2 py-0.5 rounded-full"
          style={{ color, border: `1px solid ${color}44`, background: `${color}11` }}
        >
          {proto}
        </span>
        <span className="font-mono text-[10px] text-[var(--text-secondary)]">
          {src} → {dst}
        </span>
        <span className="font-mono text-[11px] text-[var(--text-tertiary)] truncate">{amt}</span>
      </div>
      <div className="flex items-center gap-2 shrink-0">
        <span
          className={`font-mono text-[11px] ${
            profit >= 0 ? 'text-[var(--success)]' : 'text-[var(--text-tertiary)]'
          }`}
        >
          {profit >= 0 ? '+' : ''}${profit.toFixed(2)}
        </span>
        <span
          className="text-[9px] font-mono px-1.5 py-0.5 rounded"
          style={{ color: s.tone, background: `${s.tone}1a` }}
        >
          {s.label}
        </span>
      </div>
    </div>
  )
}

// ── Colosseum section ──────────────────────────────────────────────────────
function ColosseumSection() {
  return (
    <section className="border-y border-[var(--border-subtle)] bg-gradient-to-br from-[var(--brand-violet)]/5 via-transparent to-[var(--brand-cyan)]/5">
      <div className="max-w-[1200px] mx-auto px-6 py-20">
        <div className="grid md:grid-cols-[1.2fr_1fr] gap-12 items-center">
          <div>
            <Badge tone="violet">Solana Colosseum hackathon</Badge>
            <h2 className="mt-4 text-4xl font-bold tracking-tight leading-tight">
              Built so you can ship a winning bridge solver{' '}
              <span className="text-gradient-solana">this weekend.</span>
            </h2>
            <p className="mt-5 text-[var(--text-secondary)] leading-relaxed max-w-[560px]">
              We&apos;ve already done the parts that take weeks: the indexer, the genome
              stream, the protocol adapters, the lifecycle state machine. You bring the
              edge — a routing trick, a gas heuristic, a custom intent type — and the
              runtime takes care of the rest.
            </p>
            <div className="mt-7 flex flex-wrap gap-3">
              <Button href="/onboard" variant="glow">
                Start the wizard →
              </Button>
              <Button
                href="https://github.com/yawningmonsoon/taifoon-solver"
                external
                variant="secondary"
              >
                Read the source
              </Button>
            </div>
          </div>

          <div className="space-y-3">
            <Highlight
              label="Free hosted indexer"
              value="Genome SSE stream — 31 protocols, $0/mo for hackathon teams."
            />
            <Highlight
              label="On-chain registration"
              value="Auto-deploy to Base Sepolia + Solana Devnet on signup."
            />
            <Highlight
              label="One-pod-per-solver"
              value="K8s manifests included — fork or self-host."
            />
          </div>
        </div>
      </div>
    </section>
  )
}

function Highlight({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-[var(--r-md)] border border-[var(--border-default)] bg-[var(--bg-elevated)] p-4">
      <div className="text-[10px] uppercase tracking-[0.18em] text-[var(--brand-violet)] font-bold mb-1">
        {label}
      </div>
      <div className="text-sm text-[var(--text-secondary)]">{value}</div>
    </div>
  )
}

// ── Final CTA ──────────────────────────────────────────────────────────────
function FinalCTA() {
  return (
    <section className="max-w-[900px] mx-auto px-6 py-24 text-center">
      <h2 className="text-4xl md:text-5xl font-bold tracking-tight leading-tight">
        Your solver is one command away.
      </h2>
      <p className="mt-4 text-[var(--text-secondary)] max-w-[500px] mx-auto">
        The runtime is open-source. The portal is live. Hackathon credits are waiting.
      </p>
      <div className="mt-8 flex justify-center gap-3 flex-wrap">
        <Button href="/onboard" variant="glow" size="lg">
          Spin up your solver
        </Button>
        <Button href="/portal" variant="secondary" size="lg">
          Open the portal
        </Button>
      </div>
    </section>
  )
}
