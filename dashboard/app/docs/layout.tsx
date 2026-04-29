import type { Metadata } from 'next'

export const metadata: Metadata = {
  title: 'Docs — Cross-chain solver runtime overview',
  description:
    'Lightweight documentation for the Taifoon cross-chain solver runtime. CLI quick-start, architecture, and the lambda lifecycle state machine.',
  alternates: { canonical: 'https://solver.taifoon.dev/docs' },
  openGraph: {
    title: 'Docs — Cross-chain solver runtime',
    description:
      'CLI quick-start, architecture, lambda lifecycle.',
    url: 'https://solver.taifoon.dev/docs',
    type: 'article',
  },
}

export default function DocsLayout({
  children,
}: {
  children: React.ReactNode
}) {
  return children
}
