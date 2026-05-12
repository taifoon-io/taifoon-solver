'use client'

// Client-side providers for any page that needs wagmi or react-query.
// Kept thin so each page can opt in (the marketing landing page does not
// need these and shouldn't pay the bundle cost).

import { ReactNode, useState } from 'react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { WagmiProvider } from 'wagmi'
import { wagmiConfig } from './wagmi'

export function OnboardProviders({ children }: { children: ReactNode }) {
  // `useState` makes the QueryClient stable across re-renders — creating it
  // inline in the component body would reset the cache on every render.
  const [queryClient] = useState(() => new QueryClient())
  return (
    <WagmiProvider config={wagmiConfig}>
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    </WagmiProvider>
  )
}
