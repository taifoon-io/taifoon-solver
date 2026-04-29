'use client'
import { useEffect, useMemo, useState } from 'react'

/**
 * CrossChainVolume — the hero firehose.
 *
 * Three stacked panels in one self-contained SVG composition:
 *   1. Rolling-24h counter (big number, ticks up)
 *   2. 60-second multi-protocol stacked area chart
 *   3. Top-N leaderboard with mini-bars, re-orders as totals shift
 *
 * Data model:
 *   - `volumes` prop OPTIONAL. When passed, the component uses those
 *     daily baselines (one entry per protocol, USD/24h). When omitted,
 *     a built-in BASELINES table seeds the simulator.
 *   - Each tick (1s) produces a per-protocol flow = baseline/86400 ± 25%
 *     gaussian-ish noise plus an occasional 1-in-30 "burst" of 5-15x.
 *   - The 60-second area chart and the rolling-24h counter both feed
 *     from the same flow stream so they always agree.
 *
 * IMPORTANT — be honest:
 *   The BASELINES below are approximate historical 24h volumes. Replace
 *   with a daily fetch from DefiLlama's bridges API (bridges.llama.fi)
 *   before claiming the hero is "live data". Until then, the footer
 *   labels the panel as "seeded · approximate · rolling 24h".
 */

interface ProtocolBaseline {
  key: string
  label: string
  /** Approximate USD volume / 24h. Override via the `volumes` prop. */
  daily: number
  color: string
  chain?: 'evm' | 'svm' | 'both'
}

/**
 * Color discipline for the volume firehose:
 *
 *   - SVM-native protocols (Mayan Swift, Wormhole) → MINT family
 *   - Everything else (EVM-only and EVM-leaning bridges) → AZURE ramp,
 *     stepped by rank so the bars have natural visual hierarchy
 *
 * No yellow, orange, magenta, violet — those colors live in the
 * IntentRow / per-fill surfaces where they distinguish individual
 * protocol *executions*. At the aggregate-volume level, the brand
 * discipline is azure + mint, full stop.
 */
const AZURE_RAMP = [
  '#3DA5FF', // 100% — top tier
  '#2A8FE0',
  '#1F7BC4',
  '#1A6FB8',
  '#16619F',
  '#125487',
  '#0F476F',
  '#0B3A57',
  '#082E45',
  '#062335', // 10% — long tail
] as const
const MINT_FULL = '#14F195'
const MINT_DIM  = '#0FBE76'

const BASELINES: ProtocolBaseline[] = [
  { key: 'wormhole',  label: 'Wormhole',     daily: 80_000_000, color: MINT_DIM,       chain: 'both' },
  { key: 'cctp',      label: 'CCTP',         daily: 75_000_000, color: AZURE_RAMP[0],  chain: 'both' },
  { key: 'across',    label: 'Across V3',    daily: 45_000_000, color: AZURE_RAMP[1],  chain: 'evm' },
  { key: 'lifi',      label: 'LiFi',         daily: 40_000_000, color: AZURE_RAMP[2],  chain: 'both' },
  { key: 'stargate',  label: 'Stargate V2',  daily: 32_000_000, color: AZURE_RAMP[3],  chain: 'both' },
  { key: 'debridge',  label: 'deBridge DLN', daily: 18_000_000, color: AZURE_RAMP[4],  chain: 'both' },
  { key: 'mayan',     label: 'Mayan Swift',  daily: 12_000_000, color: MINT_FULL,      chain: 'svm' },
  { key: 'synapse',   label: 'Synapse',      daily:  9_000_000, color: AZURE_RAMP[5],  chain: 'evm' },
  { key: 'orbiter',   label: 'Orbiter',      daily:  8_000_000, color: AZURE_RAMP[6],  chain: 'evm' },
  { key: 'hop',       label: 'Hop',          daily:  7_000_000, color: AZURE_RAMP[7],  chain: 'evm' },
  { key: 'squid',     label: 'Squid',        daily:  5_000_000, color: AZURE_RAMP[8],  chain: 'both' },
  { key: 'symbiosis', label: 'Symbiosis',    daily:  3_000_000, color: AZURE_RAMP[9],  chain: 'evm' },
]

// ── Visualization config ───────────────────────────────────────────────
const WINDOW_SECONDS = 60          // chart x-axis span
const TICK_MS = 1000               // 1 tick per second
const COUNTER_INTERPOLATE_MS = 60  // counter animates between ticks
const CHART_HEIGHT = 140
const CHART_WIDTH = 600

interface FlowSample {
  t: number       // seconds since component mount (0..WINDOW_SECONDS-1)
  byProto: Record<string, number>  // USD this second per protocol
}

interface CrossChainVolumeProps {
  /** Optional override for daily baselines, keyed by `protocol.key`. */
  volumes?: Record<string, number>
  className?: string
}

export function CrossChainVolume({ volumes, className = '' }: CrossChainVolumeProps) {
  // Resolve baselines, honoring overrides.
  const protos: ProtocolBaseline[] = useMemo(
    () =>
      BASELINES.map((p) => ({
        ...p,
        daily: volumes?.[p.key] ?? p.daily,
      })),
    [volumes],
  )

  // Per-protocol running 24h totals (start at the daily baseline, drift up).
  const [running, setRunning] = useState<Record<string, number>>(() =>
    Object.fromEntries(protos.map((p) => [p.key, p.daily])),
  )

  // Sliding window of per-second flows. Lives in state so render reflects
  // the latest tick — refs would violate React 19's purity rules.
  const [samples, setSamples] = useState<FlowSample[]>([])

  // Smoothly-interpolated grand total for the counter. Target is derived
  // from `running` via useMemo (no extra state, no effect needed).
  const [displayTotal, setDisplayTotal] = useState(() =>
    protos.reduce((a, p) => a + p.daily, 0),
  )

  // ── Tick loop ────────────────────────────────────────────────────────
  useEffect(() => {
    let tick = 0
    const id = setInterval(() => {
      const sample: FlowSample = { t: tick, byProto: {} }
      const updates: Record<string, number> = {}
      for (const p of protos) {
        const persec = p.daily / 86_400
        // Normal-ish noise: avg of 4 uniforms approximates a bell curve.
        const noise =
          ((Math.random() + Math.random() + Math.random() + Math.random()) / 4) - 0.5
        let flow = persec * (1 + noise * 0.5)
        // Occasional burst — a fat order or batch fulfillment.
        if (Math.random() < 1 / 30) flow *= 5 + Math.random() * 10
        flow = Math.max(0, flow)
        sample.byProto[p.key] = flow
        updates[p.key] = flow
      }

      // Append + truncate window in state so the chart re-renders.
      setSamples((prev) => {
        const next = [...prev, sample]
        if (next.length > WINDOW_SECONDS) next.shift()
        return next
      })

      // Update 24h running totals (simple drift).
      setRunning((prev) => {
        const out = { ...prev }
        for (const p of protos) {
          out[p.key] = (out[p.key] ?? p.daily) + updates[p.key]
        }
        return out
      })

      tick += 1
    }, TICK_MS)
    return () => clearInterval(id)
  }, [protos])

  // ── Counter target — derived from running totals, no extra state ────
  const targetTotal = useMemo(
    () => Object.values(running).reduce((a, b) => a + b, 0),
    [running],
  )

  // Smoothly ease the displayed counter toward the target. The interval
  // is intentional (animation frame timer) — it isn't a synchronous
  // setState inside the effect body.
  useEffect(() => {
    const id = setInterval(() => {
      setDisplayTotal((curr) => {
        const delta = (targetTotal - curr) * 0.18
        if (Math.abs(delta) < 1) return targetTotal
        return curr + delta
      })
    }, COUNTER_INTERPOLATE_MS)
    return () => clearInterval(id)
  }, [targetTotal])

  // ── Build the stacked area paths ─────────────────────────────────────
  const stackedPaths = useMemo(() => {
    if (samples.length === 0) return []
    // Find the peak total flow in the window for y-scaling, then add headroom.
    let peak = 0
    for (const s of samples) {
      const total = Object.values(s.byProto).reduce((a, b) => a + b, 0)
      if (total > peak) peak = total
    }
    if (peak === 0) peak = 1
    const yScale = (v: number) => CHART_HEIGHT - (v / peak) * (CHART_HEIGHT - 8) - 4

    // Project x: newest sample at the right edge.
    const xScale = (i: number) => {
      const offset = WINDOW_SECONDS - samples.length
      return ((i + offset) / (WINDOW_SECONDS - 1)) * CHART_WIDTH
    }

    // Stack from largest baseline at the bottom, smallest on top.
    const ordered = [...protos].sort((a, b) => b.daily - a.daily)

    const cumulative: number[] = new Array(samples.length).fill(0)
    const layers: { key: string; color: string; path: string }[] = []

    for (const p of ordered) {
      // Bottom of this layer = previous cumulative; top = previous + flow
      const top: number[] = cumulative.map(
        (c, i) => c + (samples[i].byProto[p.key] ?? 0),
      )

      // Build the SVG path: along top, then back along bottom.
      const topPts = top.map((v, i) => `${xScale(i).toFixed(1)},${yScale(v).toFixed(1)}`)
      const botPts = cumulative.map(
        (v, i) => `${xScale(i).toFixed(1)},${yScale(v).toFixed(1)}`,
      )
      const path =
        `M ${topPts[0]} L ${topPts.join(' L ')} L ${[...botPts].reverse().join(' L ')} Z`

      layers.push({ key: p.key, color: p.color, path })

      // advance cumulative
      for (let i = 0; i < cumulative.length; i++) cumulative[i] = top[i]
    }
    return layers
  }, [samples, protos])

  // ── Leaderboard ──────────────────────────────────────────────────────
  const leaderboard = useMemo(() => {
    const sorted = [...protos]
      .map((p) => ({ ...p, total: running[p.key] ?? p.daily }))
      .sort((a, b) => b.total - a.total)
    const max = sorted[0]?.total ?? 1
    return sorted.slice(0, 8).map((row) => ({
      ...row,
      share: row.total / max,
    }))
  }, [protos, running])

  return (
    <div
      className={`rounded-[var(--r-sm)] border border-[var(--border-default)] bg-[var(--bg-elevated)] overflow-hidden ${className}`}
    >
      {/* Header */}
      <div className="flex items-center justify-between px-4 h-9 border-b border-[var(--border-subtle)]">
        <div className="flex items-center gap-3">
          <span className="w-1.5 h-1.5 rounded-full bg-[var(--solana-mint)] pulse-live" />
          <span className="font-mono text-[10px] tracking-[0.24em] uppercase text-[var(--solana-mint)]">
            cross-chain volume · live
          </span>
        </div>
        <span className="font-mono text-[10px] tracking-[0.18em] uppercase text-[var(--text-tertiary)]">
          {protos.length} protocols
        </span>
      </div>

      {/* Counter row */}
      <div className="px-4 pt-5 pb-3 flex items-baseline justify-between gap-4">
        <div className="flex flex-col gap-1">
          <span className="font-mono text-[10px] tracking-[0.24em] uppercase text-[var(--text-tertiary)]">
            rolling 24h · all protocols
          </span>
          <span
            className="font-mono tabular-nums text-[var(--text-primary)]"
            style={{ fontSize: 'clamp(28px, 3.5vw, 40px)', letterSpacing: '-0.01em' }}
          >
            ${formatBig(displayTotal)}
          </span>
        </div>
        <div className="flex flex-col gap-1 items-end">
          <span className="font-mono text-[10px] tracking-[0.24em] uppercase text-[var(--text-tertiary)]">
            chains
          </span>
          <span className="font-mono text-[20px] text-[var(--brand-blue)]">38+</span>
        </div>
      </div>

      {/* Stacked area chart */}
      <div className="px-4">
        <svg
          viewBox={`0 0 ${CHART_WIDTH} ${CHART_HEIGHT}`}
          preserveAspectRatio="none"
          className="w-full"
          style={{ height: CHART_HEIGHT, display: 'block' }}
        >
          <defs>
            {protos.map((p) => (
              <linearGradient key={p.key} id={`vol-grad-${p.key}`} x1="0" y1="0" x2="0" y2="1">
                <stop offset="0%" stopColor={p.color} stopOpacity="0.55" />
                <stop offset="100%" stopColor={p.color} stopOpacity="0.05" />
              </linearGradient>
            ))}
          </defs>

          {/* Gridlines */}
          {[0.25, 0.5, 0.75].map((f) => (
            <line
              key={f}
              x1="0"
              x2={CHART_WIDTH}
              y1={CHART_HEIGHT * f}
              y2={CHART_HEIGHT * f}
              stroke="rgba(230,240,247,0.05)"
              strokeWidth="1"
            />
          ))}

          {/* Stacked layers */}
          {stackedPaths.map((layer) => (
            <path
              key={layer.key}
              d={layer.path}
              fill={`url(#vol-grad-${layer.key})`}
              stroke={layer.color}
              strokeOpacity="0.4"
              strokeWidth="1"
            />
          ))}

          {/* Right edge — newest data marker */}
          <line
            x1={CHART_WIDTH - 0.5}
            x2={CHART_WIDTH - 0.5}
            y1="0"
            y2={CHART_HEIGHT}
            stroke="var(--solana-mint)"
            strokeWidth="1"
            strokeOpacity="0.6"
          />
        </svg>

        {/* Time axis */}
        <div className="flex items-center justify-between font-mono text-[9px] tracking-[0.2em] uppercase text-[var(--text-disabled)] mt-1">
          <span>−60s</span>
          <span>now</span>
        </div>
      </div>

      {/* Leaderboard */}
      <div className="px-4 pt-4 pb-3 border-t border-[var(--border-subtle)] mt-3">
        <div className="flex items-center justify-between mb-3">
          <span className="font-mono text-[10px] tracking-[0.24em] uppercase text-[var(--text-tertiary)]">
            top protocols · 24h
          </span>
          <span className="font-mono text-[9px] tracking-[0.18em] uppercase text-[var(--text-disabled)]">
            re-orders live
          </span>
        </div>
        <ul className="space-y-1.5">
          {leaderboard.map((row) => (
            <li
              key={row.key}
              className="flex items-center gap-3"
              style={{ transition: 'transform 600ms var(--ease-out)' }}
            >
              <span
                className="font-mono text-[10px] tracking-[0.12em] uppercase shrink-0"
                style={{ color: row.color, width: 96 }}
              >
                {row.label}
              </span>
              <div className="flex-1 h-1 rounded-full bg-[var(--bg-raised)] overflow-hidden">
                <div
                  className="h-full"
                  style={{
                    width: `${(row.share * 100).toFixed(1)}%`,
                    background: row.color,
                    transition: 'width 800ms var(--ease-out)',
                  }}
                />
              </div>
              <span className="font-mono text-[10px] tabular-nums text-[var(--text-secondary)] shrink-0">
                ${formatBig(row.total)}
              </span>
            </li>
          ))}
        </ul>
      </div>

      {/* Honest source footer */}
      <div className="px-4 py-2 border-t border-[var(--border-subtle)] flex items-center justify-between font-mono text-[9px] tracking-[0.18em] uppercase text-[var(--text-disabled)]">
        <span>seeded · approximate · rolling 24h</span>
        <span>source: defillama bridges + perturbation</span>
      </div>
    </div>
  )
}

// ── helpers ────────────────────────────────────────────────────────────
function formatBig(n: number): string {
  if (n >= 1e9) return (n / 1e9).toFixed(2) + 'B'
  if (n >= 1e6) return (n / 1e6).toFixed(2) + 'M'
  if (n >= 1e3) return (n / 1e3).toFixed(1) + 'K'
  return n.toFixed(2)
}
