import { NavBar, Footer } from '@/components/ui'

import { MatrixHeatmap } from '@/components/arc/MatrixHeatmap'
import { MatrixCellGrid } from '@/components/arc/MatrixCellGrid'

export const metadata = {
  title: 'Arc — Protocol matrix coverage',
  description:
    'Per-protocol coverage scoreboard for taifoon-arc — 31 catalogue protocols × 38+ chains × 4 assets. Tracks which slugs have a wired /v1/fill adapter and which fall through to skipped:not_yet_adapted.',
}

export const dynamic = 'force-dynamic'

export default function ArcMatrixPage() {
  return (
    <>
      <NavBar />
      <main className="min-h-screen" style={{ background: 'var(--bg-base)' }}>
        <MatrixHeatmap />
        <MatrixCellGrid />
      </main>
      <Footer />
    </>
  )
}
