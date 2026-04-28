/**
 * solver.taifoon.dev — Design tokens (TypeScript export)
 * Mirrors the CSS custom properties in app/globals.css.
 *
 * Use these when:
 *  - You need a color in inline styles for dynamic values (e.g. protocol pills).
 *  - You're computing animation durations programmatically.
 *
 * Prefer `var(--token-name)` in CSS / className wherever possible.
 */

export const tokens = {
  bg: {
    base: 'var(--bg-base)',
    elevated: 'var(--bg-elevated)',
    raised: 'var(--bg-raised)',
    overlay: 'var(--bg-overlay)',
  },
  border: {
    subtle: 'var(--border-subtle)',
    default: 'var(--border-default)',
    strong: 'var(--border-strong)',
  },
  text: {
    primary: 'var(--text-primary)',
    secondary: 'var(--text-secondary)',
    tertiary: 'var(--text-tertiary)',
    disabled: 'var(--text-disabled)',
  },
  brand: {
    cyan: '#00D9FF',
    cyanDim: '#00A6C4',
    violet: '#9945FF',
    violetDim: '#7A33D6',
    glow: '#14F195',
  },
  semantic: {
    success: '#00FF88',
    warning: '#FFB800',
    danger: '#FF3366',
    info: '#00D9FF',
  },
} as const

export const protocolColors: Record<string, string> = {
  across: '#00D9FF',
  debridge: '#FF6B2B',
  dln: '#FF6B2B',
  mayan: '#9945FF',
  lifi: '#F1C40F',
  orbiter: '#95A5A6',
  stargate: '#1AC8ED',
  t3rn: '#14F195',
  wormhole: '#FE5E68',
  cctp: '#2775CA',
  hop: '#B45BFF',
  connext: '#7B61FF',
  synapse: '#FF00BD',
  celer: '#00BFA6',
  axelar: '#1F2330',
  hyperlane: '#FF7DBC',
  layerzero: '#A0A0B0',
  socket: '#5A6FF0',
  squid: '#00C2A8',
  rango: '#FF8A4C',
  symbiosis: '#3FE0C2',
  meson: '#7AE2FF',
  allbridge: '#9CB6FF',
  router: '#FFA94D',
  ccip: '#0B5BD1',
}

export const chainNames: Record<number, string> = {
  1: 'Ethereum',
  10: 'Optimism',
  56: 'BSC',
  137: 'Polygon',
  8453: 'Base',
  42161: 'Arbitrum',
  43114: 'Avalanche',
  324: 'zkSync',
  59144: 'Linea',
  534352: 'Scroll',
  100: 'Gnosis',
  250: 'Fantom',
  // Solana-flavored synthetic IDs the indexer uses
  900: 'Solana',
  901: 'Solana Devnet',
}

export const chainShort: Record<number, string> = {
  1: 'ETH',
  10: 'OP',
  56: 'BSC',
  137: 'POL',
  8453: 'BASE',
  42161: 'ARB',
  43114: 'AVAX',
  324: 'ZK',
  59144: 'LIN',
  534352: 'SCR',
  100: 'GNO',
  250: 'FTM',
  900: 'SOL',
  901: 'SOL-D',
}
