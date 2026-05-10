'use client'

import Link from 'next/link'
import { NavBar, Footer } from '@/components/ui'

// ── Open routes data ───────────────────────────────────────────────────────
const OPEN_ROUTES = [
  {
    id: 'b-hyperlane-001',
    protocol: 'Hyperlane',
    chain: 'EVM → Any',
    volumeClass: 'M',
    status: 'open',
    description: 'ISM-verified cross-chain messages. mailbox.dispatch() fill path.',
  },
  {
    id: 'b-lifi-002',
    protocol: 'LiFi v2',
    chain: 'EVM multi-chain',
    volumeClass: 'M',
    status: 'open',
    description: 'Diamond router aggregator. Underlying adapter pass-through.',
  },
  {
    id: 'b-squid-001',
    protocol: 'Squid Router',
    chain: 'EVM → Cosmos',
    volumeClass: 'S',
    status: 'open',
    description: 'Axelar-backed cross-chain swaps. routeAndExecute() calldata.',
  },
  {
    id: 'b-ccip-001',
    protocol: 'Chainlink CCIP',
    chain: 'EVM → EVM',
    volumeClass: 'M',
    status: 'open',
    description: 'CCIP router message relay. ccipSend() + offramp validation.',
  },
  {
    id: 'b-stargate-002',
    protocol: 'Stargate V2',
    chain: 'EVM → EVM',
    volumeClass: 'L',
    status: 'open',
    description: 'LayerZero V2 endpoint. ride() + sendToken() via pool bus.',
  },
]

const VOLUME_BADGE: Record<string, { label: string; color: string }> = {
  S: { label: '< $1M / day', color: 'rgba(230,240,247,0.08)' },
  M: { label: '$1M–$10M / day', color: 'rgba(61,165,255,0.12)' },
  L: { label: '> $10M / day', color: 'rgba(20,241,149,0.12)' },
}

// ── Page ───────────────────────────────────────────────────────────────────
export default function BountiesPage() {
  return (
    <div className="min-h-screen flex flex-col bg-[var(--bg-base)]">
      <NavBar />

      <main className="flex-1 max-w-[1200px] mx-auto px-6 py-16 w-full">

        {/* ── TSUL banner — Volt-dominant ─────────────────────────────── */}
        <TsulBanner />

        {/* ── Section header ──────────────────────────────────────────── */}
        <div className="mt-16 mb-8 flex items-baseline justify-between flex-wrap gap-4">
          <div>
            <p
              className="font-mono text-[11px] tracking-[0.25em] mb-2"
              style={{ color: 'var(--solana-mint)' }}
            >
              [ OPEN ROUTES ]
            </p>
            <h2
              className="text-[2rem] font-light tracking-tight"
              style={{ color: 'var(--text-primary)' }}
            >
              Routes waiting for a builder
            </h2>
          </div>
          <p
            className="text-[0.875rem] max-w-sm"
            style={{ color: 'var(--text-secondary)' }}
          >
            Each route is co-owned under TSUL. Ship the adapter, pass two reviewer
            agents, merge — and collect your share of every settled call forever.
          </p>
        </div>

        {/* ── Route cards ─────────────────────────────────────────────── */}
        <div className="flex flex-col gap-4">
          {OPEN_ROUTES.map((r) => (
            <RouteCard key={r.id} route={r} />
          ))}
        </div>

        {/* ── How it works ────────────────────────────────────────────── */}
        <HowItWorks />

      </main>

      <Footer />
    </div>
  )
}

// ── TSUL Banner ────────────────────────────────────────────────────────────
function TsulBanner() {
  return (
    <div
      className="rounded-[var(--r-md)] p-px"
      style={{ background: 'var(--solana-mint)' }}
    >
      <div
        className="rounded-[var(--r-md)] px-8 py-8"
        style={{
          background: 'linear-gradient(135deg, rgba(20,241,149,0.07) 0%, var(--bg-elevated) 60%)',
        }}
      >
        {/* Pill label */}
        <div className="flex items-center gap-3 mb-6 flex-wrap">
          <span
            className="inline-flex items-center gap-2 px-3 py-1 rounded-full font-mono text-[11px] tracking-[0.2em]"
            style={{
              background: 'rgba(20,241,149,0.15)',
              color: 'var(--solana-mint)',
              border: '1px solid rgba(20,241,149,0.35)',
            }}
          >
            <span
              className="w-1.5 h-1.5 rounded-full"
              style={{ background: 'var(--solana-mint)', boxShadow: '0 0 6px #14F195' }}
            />
            TSUL · LIVE · ON-CHAIN ENFORCEMENT
          </span>
          <span
            className="font-mono text-[11px] tracking-[0.18em]"
            style={{ color: 'rgba(20,241,149,0.5)' }}
          >
            TAIFOON SUSTAINABLE USE LICENSE v1.0
          </span>
        </div>

        {/* Value headline */}
        <h1
          className="text-[1.75rem] sm:text-[2.25rem] font-light tracking-tight leading-[1.15] mb-4"
          style={{ color: 'var(--text-primary)' }}
        >
          Ship a cross-chain adapter.{' '}
          <span
            className="font-normal"
            style={{ color: 'var(--solana-mint)' }}
          >
            70% of every settled call routes to your wallet
          </span>
          {' '}— perpetually.
        </h1>

        {/* Plain-English framing */}
        <p
          className="text-[1rem] leading-relaxed mb-6 max-w-2xl"
          style={{ color: 'var(--text-secondary)' }}
        >
          No prize pool. No token cliff. No upfront.
          Under TSUL, every adapter you ship is permanently co-owned — the on-chain
          donut routes{' '}
          <span style={{ color: 'var(--solana-mint)', fontWeight: 500 }}>
            70 % to the creator, 20 % to reviewer agents, 10 % to the ecosystem
          </span>{' '}
          from the merge block forward. The{' '}
          <code
            className="font-mono text-[0.875rem] px-1.5 py-0.5 rounded"
            style={{ background: 'rgba(20,241,149,0.1)', color: 'var(--solana-mint)' }}
          >
            BuildersRegistry.recordRevenueTouch()
          </code>{' '}
          call is on-chain and irrevocable — removing it breaks the license.
        </p>

        {/* CTA row */}
        <div className="flex items-center gap-4 flex-wrap">
          <a
            href="https://github.com/yawningmonsoon/taifoon-solver/blob/master/LICENSE.md"
            target="_blank"
            rel="noreferrer"
            className="inline-flex items-center gap-2 px-5 h-10 rounded-[var(--r-sm)] font-mono text-[13px] tracking-[0.12em] transition-all"
            style={{
              background: 'var(--solana-mint)',
              color: '#000',
              fontWeight: 600,
            }}
          >
            LICENSE.md ↗
          </a>
          <a
            href="https://taifoon.io/legal/tsul"
            target="_blank"
            rel="noreferrer"
            className="inline-flex items-center gap-2 px-5 h-10 rounded-[var(--r-sm)] font-mono text-[13px] tracking-[0.12em] transition-all"
            style={{
              border: '1px solid rgba(20,241,149,0.4)',
              color: 'var(--solana-mint)',
            }}
            onMouseEnter={(e) => {
              e.currentTarget.style.background = 'rgba(20,241,149,0.08)'
              e.currentTarget.style.borderColor = 'var(--solana-mint)'
            }}
            onMouseLeave={(e) => {
              e.currentTarget.style.background = 'transparent'
              e.currentTarget.style.borderColor = 'rgba(20,241,149,0.4)'
            }}
          >
            FAQ → HOW TSUL WORKS
          </a>
          <a
            href="https://taifoon.io/os/submit-job"
            target="_blank"
            rel="noreferrer"
            className="inline-flex items-center gap-2 px-5 h-10 rounded-[var(--r-sm)] font-mono text-[13px] tracking-[0.12em] transition-colors"
            style={{ color: 'var(--text-secondary)' }}
            onMouseEnter={(e) => { e.currentTarget.style.color = 'var(--text-primary)' }}
            onMouseLeave={(e) => { e.currentTarget.style.color = 'var(--text-secondary)' }}
          >
            SUBMIT A NEW ROUTE →
          </a>
        </div>
      </div>
    </div>
  )
}

// ── Route card ─────────────────────────────────────────────────────────────
function RouteCard({ route }: { route: typeof OPEN_ROUTES[number] }) {
  const vol = VOLUME_BADGE[route.volumeClass]
  return (
    <div
      className="rounded-[var(--r-md)] px-6 py-5 flex flex-col sm:flex-row sm:items-center gap-4 transition-all group"
      style={{
        background: 'var(--bg-elevated)',
        border: '1px solid var(--border-subtle)',
      }}
      onMouseEnter={(e) => {
        const el = e.currentTarget as HTMLElement
        el.style.borderColor = 'rgba(20,241,149,0.25)'
        el.style.background = 'var(--bg-raised)'
      }}
      onMouseLeave={(e) => {
        const el = e.currentTarget as HTMLElement
        el.style.borderColor = 'var(--border-subtle)'
        el.style.background = 'var(--bg-elevated)'
      }}
    >
      {/* Left: protocol + chain */}
      <div className="flex-1 min-w-0">
        <div className="flex items-center gap-3 mb-1 flex-wrap">
          <span
            className="font-medium text-[1rem]"
            style={{ color: 'var(--text-primary)' }}
          >
            {route.protocol}
          </span>
          <span
            className="font-mono text-[11px] tracking-[0.15em] px-2 py-0.5 rounded"
            style={{ background: 'var(--bg-overlay)', color: 'var(--text-tertiary)' }}
          >
            {route.chain}
          </span>
        </div>
        <p className="text-[0.875rem]" style={{ color: 'var(--text-secondary)' }}>
          {route.description}
        </p>
      </div>

      {/* Right: volume class + TSUL sash + CTA */}
      <div className="flex items-center gap-3 shrink-0 flex-wrap">
        <span
          className="font-mono text-[11px] tracking-[0.12em] px-2.5 py-1 rounded"
          style={{ background: vol.color, color: 'var(--text-secondary)' }}
        >
          {vol.label}
        </span>
        <span
          className="font-mono text-[10px] tracking-[0.18em] px-2.5 py-1 rounded"
          style={{
            background: 'rgba(20,241,149,0.08)',
            color: 'var(--solana-mint)',
            border: '1px solid rgba(20,241,149,0.2)',
          }}
        >
          TSUL · perf-only · no upfront
        </span>
        <a
          href="https://taifoon.io/os/submit-job"
          target="_blank"
          rel="noreferrer"
          className="font-mono text-[12px] tracking-[0.12em] px-4 h-8 inline-flex items-center rounded-[var(--r-sm)] transition-all"
          style={{
            border: '1px solid rgba(20,241,149,0.3)',
            color: 'var(--solana-mint)',
          }}
          onMouseEnter={(e) => {
            e.currentTarget.style.background = 'rgba(20,241,149,0.1)'
            e.currentTarget.style.borderColor = 'var(--solana-mint)'
          }}
          onMouseLeave={(e) => {
            e.currentTarget.style.background = 'transparent'
            e.currentTarget.style.borderColor = 'rgba(20,241,149,0.3)'
          }}
        >
          CLAIM ROUTE →
        </a>
      </div>
    </div>
  )
}

// ── How it works ───────────────────────────────────────────────────────────
function HowItWorks() {
  const steps = [
    {
      n: '01',
      title: 'Ship the adapter',
      body: 'Clone the template under templates/adapter-v1. Implement quote() + fill(). Submit via BuildersRegistry.submitAdapter() — your wallet is the creator address.',
    },
    {
      n: '02',
      title: 'Two reviewer agents replay it',
      body: 'open-mamba dispatches two independent chain-replay reviewers. They run your adapter against the canonical fixture set and sign a verdict. No human gating.',
    },
    {
      n: '03',
      title: 'Auto-merge after 24h challenge window',
      body: 'Two PASS verdicts + 24 hours with no counter-example → auto-merge. From that block forward, donut accrues.',
    },
    {
      n: '04',
      title: '70% perpetually to your wallet',
      body: 'BuildersRegistry.recordRevenueTouch() fires on every settled call. Pull any time via claim(). On-chain, irrevocable, no cliff.',
    },
  ]

  return (
    <div className="mt-16">
      <p
        className="font-mono text-[11px] tracking-[0.25em] mb-4"
        style={{ color: 'var(--text-tertiary)' }}
      >
        [ HOW IT WORKS ]
      </p>
      <div className="grid sm:grid-cols-2 lg:grid-cols-4 gap-4">
        {steps.map((s) => (
          <div
            key={s.n}
            className="rounded-[var(--r-md)] px-5 py-5"
            style={{ background: 'var(--bg-elevated)', border: '1px solid var(--border-subtle)' }}
          >
            <span
              className="font-mono text-[2rem] font-light block mb-3"
              style={{ color: 'rgba(20,241,149,0.25)' }}
            >
              {s.n}
            </span>
            <p className="font-medium text-[0.9375rem] mb-2" style={{ color: 'var(--text-primary)' }}>
              {s.title}
            </p>
            <p className="text-[0.8125rem] leading-relaxed" style={{ color: 'var(--text-secondary)' }}>
              {s.body}
            </p>
          </div>
        ))}
      </div>
    </div>
  )
}
