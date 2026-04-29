import type { Metadata } from 'next'

export const metadata: Metadata = {
  title: 'Portal — Solver fleet · Live monitor',
  description:
    'Operator portal for the Taifoon cross-chain solver runtime. Spin up new solver pods, monitor existing ones, watch fills land in real time across 31 protocols and 38+ chains.',
  alternates: { canonical: 'https://solver.taifoon.dev/portal' },
  openGraph: {
    title: 'Portal — Solver fleet · Live monitor',
    description:
      'Spin up new solver pods, monitor existing ones, watch fills land in real time.',
    url: 'https://solver.taifoon.dev/portal',
    type: 'website',
  },
  // Per-solver routes are dynamic and shouldn't be indexed individually.
  robots: { index: true, follow: true },
}

export default function PortalLayout({
  children,
}: {
  children: React.ReactNode
}) {
  return children
}
