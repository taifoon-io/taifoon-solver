import type { Metadata } from 'next'
import { Inter, JetBrains_Mono } from 'next/font/google'
import './globals.css'

const inter = Inter({
  variable: '--font-inter',
  subsets: ['latin'],
  display: 'swap',
})

const jetbrainsMono = JetBrains_Mono({
  variable: '--font-jetbrains-mono',
  subsets: ['latin'],
  display: 'swap',
})

export const metadata: Metadata = {
  title: 'Taifoon Solver — Cross-chain solver runtime',
  description:
    'The fastest open-source solver runtime. Spin up a profitable cross-chain solver in minutes — Solana, EVM, 31 protocols.',
  metadataBase: new URL('https://solver.taifoon.dev'),
  openGraph: {
    title: 'Taifoon Solver — Cross-chain solver runtime',
    description:
      'Open-source solver for Across, deBridge, Mayan Swift, LiFi, Stargate and 26 more protocols.',
    url: 'https://solver.taifoon.dev',
    siteName: 'solver.taifoon.dev',
    type: 'website',
  },
}

export default function RootLayout({
  children,
}: Readonly<{ children: React.ReactNode }>) {
  return (
    <html
      lang="en"
      className={`${inter.variable} ${jetbrainsMono.variable} h-full antialiased`}
      suppressHydrationWarning
    >
      <body className="min-h-full flex flex-col font-sans bg-[var(--bg-base)] text-[var(--text-primary)]">
        {children}
      </body>
    </html>
  )
}
