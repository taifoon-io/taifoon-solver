// Wagmi config for the onboard flow.
//
// We pin Ethereum mainnet (chainId 1) as the SIWE chain — actual transactions
// happen on whatever chain the user picks during operation. The SIWE-side
// chain_id is just a replay-protection scope, kept identical to the
// `SIWE_CHAIN_ID` const in `crates/solver-api/src/hosting.rs`.
//
// Connectors:
//   - `injected()` covers MetaMask, Rabby, Brave, OKX, etc.
//   - `walletConnect` is optional — only enabled when a project id is set,
//     since WC v2 hard-fails without one. Drop a NEXT_PUBLIC_WC_PROJECT_ID
//     in `.env.local` to turn it on.

import { createConfig, http } from 'wagmi'
import { mainnet } from 'wagmi/chains'
import { injected, walletConnect } from 'wagmi/connectors'

const wcProjectId = process.env.NEXT_PUBLIC_WC_PROJECT_ID

export const wagmiConfig = createConfig({
  chains: [mainnet],
  connectors: [
    injected({ shimDisconnect: true }),
    ...(wcProjectId
      ? [
          walletConnect({
            projectId: wcProjectId,
            metadata: {
              name: 'Taifoon Solvers',
              description: 'Connect your wallet to provision a Taifoon solver pod.',
              url: 'https://solver.taifoon.dev',
              icons: ['https://solver.taifoon.dev/og.png'],
            },
            showQrModal: true,
          }),
        ]
      : []),
  ],
  transports: {
    [mainnet.id]: http(),
  },
  ssr: true,
})

export const SIWE_CHAIN_ID = mainnet.id
export const SIWE_DOMAIN = 'solver.taifoon.dev'
