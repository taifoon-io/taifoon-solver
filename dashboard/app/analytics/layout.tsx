import type { Metadata } from 'next'

export const metadata: Metadata = {
  title: 'Analytics',
  description:
    'Per-protocol P&L breakdown, chain flow matrix, fill rate trends, and outcome history for the Taifoon solver.',
  robots: { index: true, follow: true },
}

export default function AnalyticsLayout({ children }: { children: React.ReactNode }) {
  return <>{children}</>
}
