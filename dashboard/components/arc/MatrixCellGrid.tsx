'use client'

/**
 * MatrixCellGrid — per-cell heatmap of the protocol × chain × asset matrix.
 *
 * Rows: protocols (sorted by coverage desc). Columns: dst chain ids.
 * Each cell shows a colored dot per (protocol, dst, asset) cached verdict.
 * Hover → verdict + asset + last-checked. Built on /v1/matrix/cells/verdicts.
 */

import { useMemo, useState } from 'react'

import { Card, Tag } from '@/components/ui'

import { useCellVerdicts, verdictTone, type CellVerdict } from '@/lib/arc/verdicts'

function toneColor(tone: ReturnType<typeof verdictTone>): string {
  switch (tone) {
    case 'mint':  return 'var(--solana-mint)'
    case 'amber': return 'var(--warning)'
    case 'blue':  return 'var(--brand-blue)'
    default:      return 'rgba(230,240,247,0.18)'
  }
}

function timeAgo(ts: number): string {
  const dt = Math.max(0, Math.floor(Date.now() / 1000) - ts)
  if (dt < 60)   return `${dt}s`
  if (dt < 3600) return `${Math.floor(dt / 60)}m`
  return `${Math.floor(dt / 3600)}h`
}

export function MatrixCellGrid() {
  const { loading, data, error, fetchedAt } = useCellVerdicts()
  const [hover, setHover] = useState<CellVerdict | null>(null)

  // ── Group cells: (protocol, dst) → list<CellVerdict> ──────────────
  const grid = useMemo(() => {
    if (!data) return { protocols: [], dsts: [], byProtoDst: new Map<string, CellVerdict[]>() }
    const protos = new Set<string>()
    const dsts   = new Set<number>()
    const byProtoDst = new Map<string, CellVerdict[]>()
    for (const c of data.cells) {
      protos.add(c.protocol)
      dsts.add(c.dst)
      const key = `${c.protocol}|${c.dst}`
      if (!byProtoDst.has(key)) byProtoDst.set(key, [])
      byProtoDst.get(key)!.push(c)
    }
    const protocols = Array.from(protos).sort()
    const dstList   = Array.from(dsts).sort((a, b) => a - b)
    return { protocols, dsts: dstList, byProtoDst }
  }, [data])

  if (loading && !data) {
    return (
      <section className="mx-auto max-w-6xl px-6 pb-12" data-testid="cell-grid">
        <Card padding="md"><Tag tone="muted">loading per-cell verdicts…</Tag></Card>
      </section>
    )
  }
  if (error && !data) {
    return (
      <section className="mx-auto max-w-6xl px-6 pb-12" data-testid="cell-grid">
        <Card padding="md"><Tag tone="muted">{`could not load /v1/matrix/cells/verdicts: ${error}`}</Tag></Card>
      </section>
    )
  }
  if (!data) return null

  const total = data.summary.total
  if (total === 0) {
    return (
      <section className="mx-auto max-w-6xl px-6 pb-12" data-testid="cell-grid">
        <Card padding="md">
          <Tag tone="muted">no cached verdicts yet — run matrix-pump or wait for the hourly cron</Tag>
        </Card>
      </section>
    )
  }

  return (
    <section className="mx-auto max-w-6xl px-6 pb-12" data-testid="cell-grid">
      <Card padding="md">
        <div className="flex items-baseline gap-4 flex-wrap mb-4">
          <Tag tone="blue">{`Per-cell heatmap · ${total} cached`}</Tag>
          <span className="font-mono text-[10px]" style={{ color: 'var(--text-tertiary)' }}>
            {data.summary.approved} approved · {Object.keys(data.summary.by_verdict).length} distinct verdicts
          </span>
          <div className="ml-auto flex items-center gap-3 text-[10px] font-mono">
            <LegendDot tone="mint" label="approved" />
            <LegendDot tone="amber" label="A:* arc reject" />
            <LegendDot tone="blue"  label="L*:* v5 reject" />
          </div>
        </div>

        <div className="overflow-x-auto">
          <table className="font-mono text-[11px]">
            <thead>
              <tr style={{ color: 'var(--text-tertiary)' }}>
                <th className="text-left sticky left-0 pr-3 py-1 z-10"
                    style={{ background: 'var(--bg-base)' }}>protocol</th>
                {grid.dsts.map((d) => (
                  <th key={d}
                      className="px-1 py-1 text-center"
                      title={`chain ${d}`}
                      style={{ minWidth: 18 }}>
                    <span className="block" style={{ writingMode: 'vertical-rl', transform: 'rotate(180deg)' }}>
                      {d}
                    </span>
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {grid.protocols.map((p) => (
                <tr key={p} className="border-t" style={{ borderColor: 'var(--border-subtle)' }}>
                  <td className="sticky left-0 pr-3 py-1 z-10"
                      style={{
                        background: 'var(--bg-base)',
                        color: 'var(--text-primary)',
                      }}>
                    {p}
                  </td>
                  {grid.dsts.map((d) => {
                    const cells = grid.byProtoDst.get(`${p}|${d}`) ?? []
                    if (cells.length === 0) {
                      return (
                        <td key={d} className="px-1 py-1 text-center">
                          <span
                            className="inline-block h-1.5 w-1.5 rounded-full"
                            style={{ background: 'rgba(230,240,247,0.06)' }}
                          />
                        </td>
                      )
                    }
                    // Use the strongest verdict (approved > amber > blue > muted) per cell stack.
                    const ranked = cells.slice().sort((a, b) => {
                      const order = { mint: 0, amber: 1, blue: 2, muted: 3 } as const
                      return order[verdictTone(a.verdict, a.approved)]
                           - order[verdictTone(b.verdict, b.approved)]
                    })
                    const top = ranked[0]
                    const tone = verdictTone(top.verdict, top.approved)
                    return (
                      <td key={d} className="px-1 py-1 text-center"
                          onMouseEnter={() => setHover(top)}
                          onMouseLeave={() => setHover((h) => (h === top ? null : h))}>
                        <span
                          className="inline-block h-2 w-2 rounded-full cursor-help"
                          style={{ background: toneColor(tone) }}
                        />
                      </td>
                    )
                  })}
                </tr>
              ))}
            </tbody>
          </table>
        </div>

        {/* hover detail */}
        <div className="mt-4 min-h-[24px]">
          {hover ? (
            <div className="flex items-baseline gap-3 font-mono text-[11px]">
              <Tag tone={tagToneFor(verdictTone(hover.verdict, hover.approved))}>{hover.verdict}</Tag>
              <span style={{ color: 'var(--text-primary)' }}>{hover.protocol}</span>
              <span style={{ color: 'var(--text-secondary)' }}>
                {hover.src} → {hover.dst}
              </span>
              <span style={{ color: 'var(--text-tertiary)' }}>case · {hover.case_id}</span>
              <span className="ml-auto" style={{ color: 'var(--text-tertiary)' }}>
                {timeAgo(hover.ts)} ago
              </span>
            </div>
          ) : (
            <span className="font-mono text-[10px]" style={{ color: 'var(--text-tertiary)' }}>
              hover a cell · grid refreshes every 20s · cached on arc-api via /v1/box/review case_id
            </span>
          )}
        </div>

        {fetchedAt && (
          <p className="mt-3 text-[11px]" style={{ color: 'var(--text-tertiary)' }}>
            refreshed {new Date(fetchedAt).toLocaleTimeString()}
          </p>
        )}
      </Card>
    </section>
  )
}

function tagToneFor(t: ReturnType<typeof verdictTone>): 'mint' | 'blue' | 'muted' | 'violet' {
  switch (t) {
    case 'mint':  return 'mint'
    case 'amber': return 'violet'
    case 'blue':  return 'blue'
    default:      return 'muted'
  }
}

function LegendDot({ tone, label }: { tone: 'mint' | 'amber' | 'blue'; label: string }) {
  return (
    <span className="inline-flex items-center gap-1.5" style={{ color: 'var(--text-tertiary)' }}>
      <span
        className="inline-block h-1.5 w-1.5 rounded-full"
        style={{ background: toneColor(tone) }}
      />
      {label}
    </span>
  )
}
