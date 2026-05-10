import type { Metadata } from 'next'

export const metadata: Metadata = {
  title: 'Open Routes — TSUL Bounties',
  description:
    'Ship a cross-chain adapter under TSUL. 70% of every settled call routes to your wallet — perpetually, on-chain, automatic.',
  robots: { index: true, follow: true },
}

export default function BountiesLayout({ children }: { children: React.ReactNode }) {
  return <>{children}</>
}
