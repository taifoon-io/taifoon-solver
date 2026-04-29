import type { Metadata } from 'next'

// Per-solver pages are operator-specific. Don't index them individually
// — they'd just dilute the marketing surface in search results.
export const metadata: Metadata = {
  title: 'Live monitor',
  description: 'Live solver telemetry — lambda lifecycle, P&L, latency.',
  robots: { index: false, follow: false },
}

export default function SolverMonitorLayout({
  children,
}: {
  children: React.ReactNode
}) {
  return children
}
