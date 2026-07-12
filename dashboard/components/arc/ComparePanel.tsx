'use client'

/**
 * ComparePanel — live pricing comparison for the 7 curated Live cells.
 *
 * Each cell calls /v1/compare on its own 30s cadence; the panel renders:
 *   - cell header (label + best protocol + total)
 *   - top-5 rows by total_cost_usd with fee / gas / donut surcharge / Δ
 *
 * Designed to sit next to ArcExplorer on /arc. Brand-faithful: pure-black
 * canvas, JetBrains Mono, mint highlights for best, azure for headers,
 * gap-px separators between cells.
 */

import { Card, Tag } from '@/components/ui'

import {
  CURATED_LIVE_CELLS,
  useArcCompare,
  type CompareRequest,
  type CompareRow,
} from '@/lib/arc/compare'

const TONE_BEST = '#14F195'
const TONE_BLUE = '#3DA5FF'

export function ComparePanel() {
  return (
    <section className="mx-auto max-w-6xl px-6 py-6">
      <header className="mb-4">
        <div
          className="text-[10px] uppercase font-mono tracking-[0.25em] mb-1"
          style={{ color: TONE_BLUE }}
        >
          live pricing · /v1/compare
        </div>
        <h2 className="text-lg font-medium" style={{ color: 'var(--text-primary)' }}>
          Box — best route per cell
        </h2>
        <p className="mt-2 max-w-2xl text-[12px]" style={{ color: 'var(--text-secondary)' }}>
          7 known-good Phase-4 cells re-priced every 30s. Best protocol per
          cell wins on total cost (gas + protocol fee + 49bp donut surcharge).
          Hover donut to see 70/20/10 split.
        </p>
      </header>

      <div className="grid grid-cols-1 lg:grid-cols-2 gap-px" style={{ background: 'var(--border-subtle)' }}>
        {CURATED_LIVE_CELLS.map((cell) => (
          <CompareCell key={cell.label} label={cell.label} req={cell.req} />
        ))}
      </div>
    </section>
  )
}

function CompareCell({ label, req }: { label: string; req: CompareRequest }) {
  const { loading, data, error, fetchedAt } = useArcCompare(req, 30_000)

  return (
    <Card padding="md">
      <div data-testid={`compare-cell-${label.replace(/[^a-z0-9]+/gi, '-')}`}>
        {/* ── Cell header ─────────────────────────────────────────── */}
        <header className="flex items-start justify-between gap-3 mb-2">
          <div className="min-w-0">
            <div className="text-[13px] font-mono truncate" style={{ color: 'var(--text-primary)' }}>
              {label}
            </div>
            <div className="text-[10px] font-mono mt-1" style={{ color: 'var(--text-tertiary)' }}>
              {req.src_chain_id} → {req.dst_chain_id}{req.dst_kind === 'solana' ? ' (sol)' : ''} · ${(req.input_amount / 1e6).toFixed(2)} {req.asset ?? 'USDC'}
            </div>
          </div>
          {data?.best && (
            <Tag tone="mint">{`best · ${data.best}`}</Tag>
          )}
        </header>

        {/* ── Body ─────────────────────────────────────────────────── */}
        {loading && !data && (
          <p className="text-[11px] font-mono" style={{ color: 'var(--text-tertiary)' }}>
            pricing…
          </p>
        )}

        {error && !data && (
          <p className="text-[11px] font-mono" style={{ color: 'var(--warning)' }}>
            {error}
          </p>
        )}

        {data && data.rows.length === 0 && (
          <p className="text-[11px] font-mono" style={{ color: 'var(--text-tertiary)' }}>
            no executable routes
          </p>
        )}

        {data && data.rows.length > 0 && (
          <>
            <CompareTable rows={data.rows.slice(0, 5)} />
            <p className="mt-2 text-[10px] font-mono" style={{ color: 'var(--text-tertiary)' }}>
              {data.protocol_count} protocols ·
              {fetchedAt ? ` refreshed ${new Date(fetchedAt).toLocaleTimeString()}` : ''} ·
              {' source: synthetic'}
            </p>
          </>
        )}
      </div>
    </Card>
  )
}

function CompareTable({ rows }: { rows: CompareRow[] }) {
  return (
    <table className="w-full font-mono text-[11px]">
      <thead>
        <tr style={{ color: 'var(--text-tertiary)', borderBottom: '1px solid rgba(255,255,255,0.10)' }}>
          <th className="text-left  py-1 uppercase tracking-[0.16em] font-normal">protocol</th>
          <th className="text-right py-1 uppercase tracking-[0.16em] font-normal">fee</th>
          <th className="text-right py-1 uppercase tracking-[0.16em] font-normal">gas</th>
          <th className="text-right py-1 uppercase tracking-[0.16em] font-normal" title="49bps donut surcharge — 70% contributor / 20% treasury / 10% solver">
            donut
          </th>
          <th className="text-right py-1 uppercase tracking-[0.16em] font-normal">total · Δ</th>
        </tr>
      </thead>
      <tbody>
        {rows.map((r) => <CompareRowItem key={r.protocol_slug} r={r} />)}
      </tbody>
    </table>
  )
}

function CompareRowItem({ r }: { r: CompareRow }) {
  const isBest = !!r.best
  return (
    <tr
      style={{
        borderTop: '1px solid var(--border-subtle)',
        background: isBest ? 'rgba(20,241,149,0.06)' : 'transparent',
      }}
      title={`donut split: contributor $${r.donut_split.contributor_usd.toFixed(5)} · treasury $${r.donut_split.treasury_usd.toFixed(5)} · solver $${r.donut_split.solver_usd.toFixed(5)}`}
    >
      <td className="py-1 pr-2 text-left">
        <div className="flex items-center gap-2 min-w-0">
          {isBest && (
            <span
              style={{
                color: '#0a1020', background: TONE_BEST,
                padding: '1px 4px', fontSize: 8, fontWeight: 700,
                letterSpacing: '0.16em',
              }}
            >★</span>
          )}
          <span style={{ color: isBest ? TONE_BEST : 'var(--text-primary)' }}>
            {r.protocol_slug}
          </span>
        </div>
      </td>
      <td className="py-1 text-right" style={{ color: 'var(--text-secondary)' }}>
        ${r.protocol_fee_usd.toFixed(4)}
      </td>
      <td className="py-1 text-right" style={{ color: 'var(--text-secondary)' }}>
        ${r.gas_cost_usd.toFixed(4)}
      </td>
      <td className="py-1 text-right cursor-help" style={{ color: 'var(--text-secondary)' }}>
        ${r.donut_surcharge_usd.toFixed(4)}
      </td>
      <td className="py-1 text-right">
        <div style={{ color: isBest ? TONE_BEST : 'var(--text-primary)' }}>
          ${r.total_cost_usd.toFixed(4)}
        </div>
        <div style={{ color: 'var(--text-tertiary)', fontSize: 9 }}>
          {r.delta_vs_best_usd === 0 ? '—' : `+$${r.delta_vs_best_usd.toFixed(4)}`}
        </div>
      </td>
    </tr>
  )
}
