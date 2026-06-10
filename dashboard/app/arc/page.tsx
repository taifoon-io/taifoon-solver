import { NavBar, Footer } from '@/components/ui'

import { ArcExplorer } from '@/components/arc/ArcExplorer'
import { ComparePanel } from '@/components/arc/ComparePanel'

export const metadata = {
  title: 'Arc — Live cross-chain dispatch explorer',
  description:
    'Operator view of taifoon-arc: every quote, fill, V5 review, and donut accrual on taifoon-devnet + Solana devnet in real time. SSE-driven with sub-second latency.',
}

export const dynamic = 'force-dynamic'

export default function ArcPage() {
  return (
    <>
      <NavBar />
      <main className="min-h-screen" style={{ background: 'var(--bg-base)' }}>
        <ArcExplorer />
        <ComparePanel />
      </main>
      <Footer />
    </>
  )
}
