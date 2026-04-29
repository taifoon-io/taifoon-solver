import type { Metadata } from 'next'

export const metadata: Metadata = {
  title: 'Onboarding — Spin up a cross-chain solver',
  description:
    'Four-phase wizard for spinning up a Taifoon cross-chain solver — pick chains and protocols, generate a wallet, copy the launch command. Live on Solana + EVM in under five minutes.',
  alternates: { canonical: 'https://solver.taifoon.dev/onboard' },
  openGraph: {
    title: 'Onboarding — Spin up a cross-chain solver',
    description:
      'Pick chains and protocols, generate a wallet, copy the launch command. Live on Solana + EVM in five minutes.',
    url: 'https://solver.taifoon.dev/onboard',
    type: 'website',
  },
}

export default function OnboardLayout({
  children,
}: {
  children: React.ReactNode
}) {
  return children
}
