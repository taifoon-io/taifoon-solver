'use client'

/**
 * MatrixHeatmap — protocol × chain coverage view.
 *
 * Pulls /v1/matrix/coverage and surfaces:
 *   - STATUS column with prominent pills: EXECUTABLE / CATALOGUED
 *   - "Working only" filter to hide unwired rows
 *   - Wired-first sort so executable routes appear first
 *
 * Refreshes every 30s via useMatrixCoverage.
 */

import { useMemo, useState } from 'react'

import { Card, Tag, StatTile } from '@/components/ui'

import { useMatrixCoverage, type MatrixProtocolRow } from '@/lib/arc/matrix'

export function MatrixHeatmap() {
  const { loading, data, error, fetchedAt } = useMatrixCoverage()
  const [workingOnly, setWorkingOnly] = useState(false)

  if (loading && !data) {
    return (
      <section className="mx-auto max-w-6xl px-6 py-12">
        <Tag tone="muted">loading matrix coverage…</Tag>
      </section>
    )
  }
  if (error && !data) {
    return (
      <section className="mx-auto max-w-6xl px-6 py-12">
        <Tag tone="muted">{`could not load matrix coverage: ${error}`}</Tag>
      </section>
    )
  }
  if (!data) return null

  const { summary, protocols: allProtocols, wired_adapters } = data

  // Sort by tier (executable → family_wired → catalogued) then cell count.
  // "show working only" hides anything not actually executable end-to-end.
  const tierRank = (p: MatrixProtocolRow): number => {
    const t = p.coverage_tier ?? (p.has_fill_adapter ? 'executable' : 'catalogued')
    return t === 'executable' ? 0 : t === 'family_wired' ? 1 : 2
  }
  const protocols: MatrixProtocolRow[] = useMemo(() => {
    const filtered = workingOnly
      ? allProtocols.filter((p) => tierRank(p) === 0)
      : allProtocols
    return [...filtered].sort((a, b) => {
      const da = tierRank(a), db = tierRank(b)
      if (da !== db) return da - db
      return b.matrix_cells - a.matrix_cells
    })
  }, [allProtocols, workingOnly])

  return (
    <div className="relative">
      {/* ── Header ───────────────────────────────────────────────── */}
      <section className="mx-auto max-w-6xl px-6 pt-10 pb-6 md:pt-14 md:pb-8">
        <div className="flex items-center gap-3 flex-wrap">
          <Tag tone="blue">Arc · Protocol matrix</Tag>
          <a
            href="/arc"
            className="ml-auto font-mono text-[10px] uppercase tracking-[0.25em] underline-offset-4 hover:underline"
            style={{ color: 'var(--brand-blue)' }}
          >
            ← live dispatches
          </a>
        </div>
        <h1
          className="mt-6 text-3xl md:text-5xl font-light uppercase"
          style={{
            background: 'linear-gradient(180deg, var(--text-primary) 0%, rgba(230,240,247,0.5) 100%)',
            WebkitBackgroundClip: 'text',
            WebkitTextFillColor: 'transparent',
            letterSpacing: '0.08em',
          }}
        >
          Coverage scoreboard
        </h1>
        <p className="mt-3 max-w-xl text-sm" style={{ color: 'var(--text-secondary)' }}>
          {summary.executable_protocols ?? summary.wired_protocols} of {summary.total_protocols}{' '}
          protocols are <strong style={{ color: 'var(--solana-mint)' }}>executable</strong> end-to-end.{' '}
          {(summary.family_wired_protocols ?? 0) > 0 && (
            <>
              {summary.family_wired_protocols} more are{' '}
              <strong style={{ color: 'var(--brand-blue)' }}>family-wired</strong> (dispatched
              generically via abi family).{' '}
            </>
          )}
          The remaining {summary.catalogued_protocols ?? summary.unwired_protocols} are{' '}
          <code className="font-mono">catalogued</code> (no path yet).
        </p>
      </section>

      {/* ── Summary stats ───────────────────────────────────────── */}
      <section className="mx-auto max-w-6xl px-6 pb-px" data-testid="matrix-summary">
        <div
          className="grid grid-cols-2 md:grid-cols-5 gap-px"
          style={{ background: 'var(--border-subtle)' }}
        >
          <Card padding="md"><StatTile label="protocols" value={summary.total_protocols} /></Card>
          <Card padding="md">
            <StatTile
              label="executable"
              value={summary.executable_protocols ?? summary.wired_protocols}
              tone="mint"
            />
          </Card>
          <Card padding="md">
            <StatTile
              label="family-wired"
              value={summary.family_wired_protocols ?? 0}
              tone="blue"
            />
          </Card>
          <Card padding="md">
            <StatTile
              label="catalogued"
              value={summary.catalogued_protocols ?? summary.unwired_protocols}
              tone="warning"
            />
          </Card>
          <Card padding="md">
            <StatTile
              label="exec coverage"
              value={`${summary.wired_coverage_pct.toFixed(1)}%`}
              tone={summary.wired_coverage_pct > 50 ? 'mint' : 'warning'}
            />
          </Card>
        </div>
      </section>

      {/* ── Wired adapters chips ────────────────────────────────── */}
      <section className="mx-auto max-w-6xl px-6 pb-px">
        <Card padding="md">
          <div className="flex items-center gap-3 flex-wrap">
            <Tag tone="mint">wired adapters</Tag>
            {wired_adapters.map((slug) => (
              <span
                key={slug}
                className="font-mono text-[11px] px-2 py-0.5"
                style={{
                  background: 'rgba(20,241,149,0.10)',
                  color: 'var(--solana-mint)',
                  border: '1px solid rgba(20,241,149,0.25)',
                }}
              >
                {slug}
              </span>
            ))}
          </div>
        </Card>
      </section>

      {/* ── Per-protocol grid ───────────────────────────────────── */}
      <section className="mx-auto max-w-6xl px-6 pb-12" data-testid="matrix-table">
        <Card padding="md">
          <div className="flex items-baseline gap-3 flex-wrap mb-3">
            <Tag tone="blue">{`Per-protocol · ${protocols.length} of ${allProtocols.length} shown`}</Tag>
            <button
              type="button"
              data-testid="matrix-filter-working-only"
              onClick={() => setWorkingOnly((v) => !v)}
              className="font-mono text-[10px] uppercase tracking-[0.25em] cursor-pointer"
              style={{
                color: workingOnly ? 'var(--solana-mint)' : 'var(--text-tertiary)',
                background: workingOnly ? 'rgba(20,241,149,0.10)' : 'transparent',
                border: `1px solid ${workingOnly ? 'rgba(20,241,149,0.35)' : 'var(--border-default)'}`,
                padding: '4px 10px',
              }}
            >
              {workingOnly ? '✓ working only' : '· show working only'}
            </button>
            <span className="font-mono text-[10px]" style={{ color: 'var(--text-tertiary)' }}>
              {summary.wired_protocols} executable · {summary.unwired_protocols} catalogued
            </span>
          </div>
          <table className="w-full font-mono text-xs">
            <thead>
              <tr style={{ color: 'var(--text-tertiary)', letterSpacing: '0.15em' }}>
                <th className="text-left uppercase font-normal py-2 pr-3" style={{ width: 120 }}>status</th>
                <th className="text-left uppercase font-normal pr-3">protocol</th>
                <th className="text-left uppercase font-normal pr-3">family</th>
                <th className="text-right uppercase font-normal pr-3">chains</th>
                <th className="text-right uppercase font-normal pr-3">cells</th>
                <th className="text-right uppercase font-normal pr-3">solana</th>
                <th className="text-right uppercase font-normal">fallback</th>
              </tr>
            </thead>
            <tbody data-testid="matrix-table-body">
              {protocols.map((p) => (
                <tr
                  key={p.slug}
                  className="border-t"
                  style={{ borderColor: 'var(--border-subtle)' }}
                >
                  <td className="py-2 pr-3">
                    <TierPill row={p} />
                  </td>
                  <td className="py-2 pr-3 font-mono" style={{ color: 'var(--text-primary)' }}>
                    {p.slug}
                  </td>
                  <td className="py-2 pr-3 font-mono" style={{ color: 'var(--text-secondary)' }}>
                    {p.family}
                  </td>
                  <td className="py-2 pr-3 text-right">{p.chains_supported}</td>
                  <td className="py-2 pr-3 text-right" style={{ color: 'var(--brand-blue)' }}>
                    {p.matrix_cells}
                  </td>
                  <td className="py-2 pr-3 text-right">
                    {p.solana ? (
                      <span style={{ color: 'var(--solana-mint)' }}>✓</span>
                    ) : (
                      <span style={{ color: 'var(--text-tertiary)' }}>—</span>
                    )}
                  </td>
                  <td className="py-2 text-right font-mono" style={{ color: 'var(--text-tertiary)', fontSize: 10 }}>
                    {p.fallback_behavior}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          {fetchedAt && (
            <p className="mt-3 text-[11px]" style={{ color: 'var(--text-tertiary)' }}>
              refreshed {new Date(fetchedAt).toLocaleTimeString()} · re-polls every 30s
            </p>
          )}
        </Card>
      </section>
    </div>
  )
}

/**
 * TierPill — 3-tier coverage indicator.
 *   EXECUTABLE  mint   : adapter wired and runs end-to-end
 *   FAMILY      azure  : dispatched generically via abi family
 *   CATALOGUED  amber  : known protocol, no path yet → skipped
 *
 * Falls back to has_fill_adapter for arc-api versions that don't return
 * coverage_tier (so the page still renders during a partial deploy).
 */
function TierPill({ row }: { row: MatrixProtocolRow }) {
  const tier = row.coverage_tier
    ?? (row.has_fill_adapter ? 'executable' : 'catalogued')

  if (tier === 'executable') {
    const via = row.routed_via ? ` (via ${row.routed_via})` : ''
    return (
      <span
        title={`Adapter wired into /v1/fill — executes against the protocol${via}`}
        style={{
          color: 'var(--bg-base)',
          background: 'var(--solana-mint)',
          padding: '3px 8px',
          fontSize: 10,
          fontWeight: 600,
          letterSpacing: '0.15em',
        }}
        data-testid={`tier-pill-${row.slug}`}
      >
        EXECUTABLE
      </span>
    )
  }

  if (tier === 'family_wired') {
    return (
      <span
        title="Dispatched generically via abi family — surfaces a family-tagged receipt, no adapter wired yet"
        style={{
          color: 'var(--brand-blue)',
          background: 'rgba(61,165,255,0.10)',
          padding: '3px 8px',
          fontSize: 10,
          fontWeight: 500,
          letterSpacing: '0.15em',
          border: '1px solid rgba(61,165,255,0.30)',
        }}
        data-testid={`tier-pill-${row.slug}`}
      >
        FAMILY
      </span>
    )
  }

  return (
    <span
      title="In the catalogue but no adapter — /v1/fill returns skipped:not_yet_adapted"
      style={{
        color: 'var(--warning)',
        background: 'rgba(255,180,84,0.08)',
        padding: '3px 8px',
        fontSize: 10,
        fontWeight: 500,
        letterSpacing: '0.15em',
        border: '1px solid rgba(255,180,84,0.25)',
      }}
      data-testid={`tier-pill-${row.slug}`}
    >
      CATALOGUED
    </span>
  )
}
