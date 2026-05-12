import type { Metadata } from 'next'

export const metadata: Metadata = {
  title: 'TSUL Donut Policy — Canonical fee split + adapter registry',
  description:
    'Public, auditable view of the TSUL donut policy applied uniformly across every provisioned adapter. Reads /api/donut/policy and /api/donut/registry — no source code spelunking required.',
  alternates: { canonical: 'https://solver.taifoon.dev/policy' },
  openGraph: {
    title: 'TSUL Donut Policy — Canonical fee split + adapter registry',
    description:
      'The 49 bps × 70 / 20 / 10 split, applied uniformly across every provisioned adapter — verifiable by any auditor.',
    url: 'https://solver.taifoon.dev/policy',
    type: 'website',
  },
  robots: { index: true, follow: true },
}

export default function PolicyLayout({
  children,
}: {
  children: React.ReactNode
}) {
  return children
}
