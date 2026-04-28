'use client'
/**
 * ChainOrbits — ambient SVG decoration for the hero.
 *
 * Three concentric orbits with chain nodes (Solana, Base, Arbitrum,
 * Ethereum, Optimism, Polygon, BSC, Avalanche). A faint connecting line
 * between Solana and Base periodically pulses — visual shorthand for
 * "cross-chain settlement". Pure CSS animation, no JS state, no layout
 * shift, ~3kb.
 *
 * Designed to sit absolutely-positioned behind the hero copy at low
 * opacity. Pointer-events-none so it never blocks CTAs.
 */

const ORBITS = [
  { r: 240, dur: '120s', dir: 1, count: 3 },
  { r: 340, dur: '180s', dir: -1, count: 4 },
  { r: 440, dur: '240s', dir: 1, count: 5 },
]

const NODES = [
  { orbit: 0, angle: 30, label: 'SOL', color: '#14F195', solana: true },
  { orbit: 0, angle: 150, label: 'BASE', color: '#3DA5FF' },
  { orbit: 0, angle: 270, label: 'ARB', color: '#3DA5FF' },
  { orbit: 1, angle: 0, label: 'ETH', color: '#94B0C4' },
  { orbit: 1, angle: 90, label: 'OP', color: '#FF6B6B' },
  { orbit: 1, angle: 180, label: 'POL', color: '#9945FF' },
  { orbit: 1, angle: 270, label: 'BSC', color: '#F1C40F' },
  { orbit: 2, angle: 36, label: 'AVAX', color: '#FF6B6B' },
  { orbit: 2, angle: 108, label: 'LIN', color: '#94B0C4' },
  { orbit: 2, angle: 180, label: 'ZK', color: '#94B0C4' },
  { orbit: 2, angle: 252, label: 'SCR', color: '#FFB454' },
  { orbit: 2, angle: 324, label: 'GNO', color: '#14F195' },
]

export function ChainOrbits({ className = '' }: { className?: string }) {
  const size = 1000
  const cx = size / 2
  const cy = size / 2
  return (
    <div className={`absolute inset-0 pointer-events-none ${className}`} aria-hidden="true">
      <svg
        viewBox={`0 0 ${size} ${size}`}
        className="absolute left-1/2 top-1/2 -translate-x-1/2 -translate-y-1/2"
        style={{ width: 'min(120vh, 1200px)', height: 'min(120vh, 1200px)', maxWidth: '120vw' }}
      >
        <defs>
          <radialGradient id="orbit-fade" cx="50%" cy="50%" r="50%">
            <stop offset="0%" stopColor="rgba(61,165,255,0.08)" />
            <stop offset="60%" stopColor="rgba(61,165,255,0.03)" />
            <stop offset="100%" stopColor="rgba(0,0,0,0)" />
          </radialGradient>
          <linearGradient id="link-grad" x1="0%" y1="0%" x2="100%" y2="0%">
            <stop offset="0%" stopColor="#3DA5FF" stopOpacity="0" />
            <stop offset="50%" stopColor="#14F195" stopOpacity="0.6" />
            <stop offset="100%" stopColor="#9945FF" stopOpacity="0" />
          </linearGradient>
        </defs>

        {/* Orbit rings */}
        {ORBITS.map((o, i) => (
          <circle
            key={i}
            cx={cx}
            cy={cy}
            r={o.r}
            fill="none"
            stroke="rgba(230,240,247,0.06)"
            strokeWidth="1"
            strokeDasharray="2 6"
          />
        ))}

        {/* Nodes — each in a rotating group so they move along the orbit */}
        {NODES.map((n, i) => {
          const orbit = ORBITS[n.orbit]
          const dir = orbit.dir > 0 ? 'normal' : 'reverse'
          return (
            <g
              key={i}
              style={{
                transformOrigin: `${cx}px ${cy}px`,
                animation: `orbit-spin ${orbit.dur} linear infinite ${dir}`,
                transform: `rotate(${n.angle}deg)`,
              }}
            >
              <g transform={`translate(${cx + orbit.r}, ${cy})`}>
                {/* counter-rotate the label so it stays upright */}
                <g
                  style={{
                    animation: `orbit-spin ${orbit.dur} linear infinite ${dir === 'normal' ? 'reverse' : 'normal'}`,
                    transformOrigin: '0 0',
                  }}
                >
                  <circle
                    r={n.solana ? 6 : 4}
                    fill={n.color}
                    style={{
                      filter: n.solana ? 'drop-shadow(0 0 8px ' + n.color + ')' : undefined,
                    }}
                  />
                  <text
                    x="10"
                    y="4"
                    fontSize="10"
                    fill="rgba(230,240,247,0.5)"
                    fontFamily="JetBrains Mono, monospace"
                    letterSpacing="2"
                  >
                    {n.label}
                  </text>
                </g>
              </g>
            </g>
          )
        })}

        {/* central glow */}
        <circle cx={cx} cy={cy} r="200" fill="url(#orbit-fade)" />

        {/* SOL ↔ BASE pulse line */}
        <line
          x1={cx + ORBITS[0].r * Math.cos((30 * Math.PI) / 180)}
          y1={cy + ORBITS[0].r * Math.sin((30 * Math.PI) / 180)}
          x2={cx + ORBITS[0].r * Math.cos((150 * Math.PI) / 180)}
          y2={cy + ORBITS[0].r * Math.sin((150 * Math.PI) / 180)}
          stroke="url(#link-grad)"
          strokeWidth="1"
          style={{ animation: 'link-pulse 3.5s ease-in-out infinite' }}
        />

        <style>{`
          @keyframes orbit-spin {
            from { transform: rotate(0deg); }
            to { transform: rotate(360deg); }
          }
          @keyframes link-pulse {
            0%, 100% { opacity: 0.2; }
            50%      { opacity: 0.9; }
          }
        `}</style>
      </svg>
    </div>
  )
}
