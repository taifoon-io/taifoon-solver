import type { Metadata, Viewport } from 'next'
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

const SITE_URL = 'https://solver.taifoon.dev'
const SITE_NAME = 'Taifoon Solvers'
const SITE_DESC =
  'Open-source cross-chain solver runtime for Solana and EVM. 31 protocols, 38+ chains, sub-second SSE telemetry. Spin up a profitable solver pod in minutes — built for Solana Colosseum and production market-makers alike.'

export const metadata: Metadata = {
  metadataBase: new URL(SITE_URL),
  title: {
    default: 'Taifoon Solvers — Cross-chain solver runtime · Solana + EVM',
    template: '%s · Taifoon Solvers',
  },
  description: SITE_DESC,

  // Keywords are weak signal in 2026 but harmless and surface in some
  // social previews / AI search summaries.
  keywords: [
    'cross-chain solver',
    'Solana solver',
    'intent solver',
    'bridge solver',
    'Across V3 solver',
    'deBridge DLN solver',
    'Mayan Swift',
    'Wormhole',
    'CCTP',
    'LayerZero',
    'Solana Colosseum',
    'cross-chain runtime',
    'autonomous solver',
    'Taifoon',
    'open-source solver',
  ],
  authors: [{ name: 'yawningmonsoon', url: 'https://github.com/yawningmonsoon' }],
  creator: 'Taifoon',
  publisher: 'Taifoon',

  category: 'technology',

  alternates: {
    canonical: SITE_URL,
  },

  openGraph: {
    title: 'Taifoon Solvers — Cross-chain solver runtime',
    description: SITE_DESC,
    url: SITE_URL,
    siteName: SITE_NAME,
    type: 'website',
    locale: 'en_US',
    images: [
      {
        url: '/og.png',
        width: 1200,
        height: 630,
        alt: 'Taifoon Solvers — open-source cross-chain solver runtime for Solana and EVM',
      },
    ],
  },

  twitter: {
    card: 'summary_large_image',
    title: 'Taifoon Solvers — Cross-chain solver runtime',
    description:
      'Open-source solver runtime for Solana and EVM. 31 protocols, 38+ chains, sub-second telemetry.',
    images: ['/og.png'],
    site: '@taifoon_io',
    creator: '@taifoon_io',
  },

  robots: {
    index: true,
    follow: true,
    googleBot: {
      index: true,
      follow: true,
      'max-image-preview': 'large',
      'max-snippet': -1,
      'max-video-preview': -1,
    },
  },

  applicationName: SITE_NAME,
  referrer: 'origin-when-cross-origin',

  // Apple / PWA bits — cheap to ship, helps with home-screen pinning.
  appleWebApp: {
    capable: true,
    title: 'Taifoon Solvers',
    statusBarStyle: 'black-translucent',
  },

  formatDetection: {
    email: false,
    address: false,
    telephone: false,
  },
}

export const viewport: Viewport = {
  themeColor: '#000000',
  colorScheme: 'dark',
  width: 'device-width',
  initialScale: 1,
}

export default function RootLayout({
  children,
}: Readonly<{ children: React.ReactNode }>) {
  // JSON-LD structured data — `Organization` + `SoftwareApplication`.
  // Boosts knowledge-panel pickups on Google and gives AI search
  // crawlers a clean shape to cite.
  const orgLd = {
    '@context': 'https://schema.org',
    '@type': 'Organization',
    name: 'Taifoon',
    url: 'https://taifoon.io',
    sameAs: [
      'https://github.com/yawningmonsoon/taifoon-solver',
      'https://solver.taifoon.dev',
    ],
    logo: `${SITE_URL}/og.png`,
  }

  const appLd = {
    '@context': 'https://schema.org',
    '@type': 'SoftwareApplication',
    name: SITE_NAME,
    applicationCategory: 'DeveloperApplication',
    operatingSystem: 'Linux, macOS',
    description: SITE_DESC,
    url: SITE_URL,
    offers: {
      '@type': 'Offer',
      price: '0',
      priceCurrency: 'USD',
    },
    softwareVersion: '0.1',
    license: 'https://opensource.org/licenses/MIT',
    author: {
      '@type': 'Organization',
      name: 'Taifoon',
      url: 'https://taifoon.io',
    },
    sameAs: ['https://github.com/yawningmonsoon/taifoon-solver'],
  }

  return (
    <html
      lang="en"
      className={`${inter.variable} ${jetbrainsMono.variable} h-full antialiased`}
      suppressHydrationWarning
    >
      <head>
        <script
          type="application/ld+json"
          dangerouslySetInnerHTML={{ __html: JSON.stringify(orgLd) }}
        />
        <script
          type="application/ld+json"
          dangerouslySetInnerHTML={{ __html: JSON.stringify(appLd) }}
        />
      </head>
      <body className="min-h-full flex flex-col font-sans bg-[var(--bg-base)] text-[var(--text-primary)]">
        {children}
      </body>
    </html>
  )
}
