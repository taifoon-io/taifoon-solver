/**
 * solver.taifoon.dev — Design tokens (TypeScript export)
 * Mirrors the CSS custom properties in app/globals.css.
 *
 * Aligned with taifoon.io: pure-black canvas, single azure accent #3DA5FF,
 * soft #E6F0F7 ink. Solana-flavored secondary accents (#14F195 mint,
 * #9945FF violet) used sparingly for solver-specific moments.
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
    blue: '#3DA5FF',
    blueDim: '#2787DB',
    blueSoft: 'rgba(61, 165, 255, 0.12)',
  },
  solana: {
    mint: '#14F195',
    violet: '#9945FF',
  },
  semantic: {
    success: '#14F195',
    warning: '#FFB454',
    danger: '#FF6B6B',
    info: '#3DA5FF',
  },
} as const

export const protocolColors: Record<string, string> = {
  across: '#3DA5FF',
  debridge: '#FF8A4C',
  dln: '#FF8A4C',
  mayan: '#9945FF',
  lifi: '#F1C40F',
  orbiter: '#95A5A6',
  stargate: '#6BC8FF',
  t3rn: '#14F195',
  wormhole: '#FE5E68',
  cctp: '#2775CA',
  hop: '#B45BFF',
  connext: '#7B61FF',
  synapse: '#FF00BD',
  celer: '#00BFA6',
  axelar: '#A0A0B0',
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
