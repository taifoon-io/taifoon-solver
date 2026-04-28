'use client'
/**
 * ChainOrbits — taifoon.io-aligned hero geometry.
 *
 * Three layered elements, all with locked color palette:
 *
 *   1. tf-grid          — 80px square grid at 2.4% white, tilted ~1.7°
 *                         around X via matrix3d, drifts infinitely (180s)
 *   2. orbital-trace    — outer azure ring + a square node, rotates
 *                         counter-clockwise over 90s
 *   3. orbital-trace-2  — inner azure/mint ring + a square node, rotates
 *                         clockwise over 70s, with a mint-tinted node so
 *                         it reads as the "Solana" sibling of the parent
 *                         brand without breaking the palette
 *
 * Color set, total: white-2.4%, white-3%, azure-6%, azure-12%, mint-30%.
 * Nothing else. The discipline is the point.
 *
 * Pure CSS animations — no JS state, no layout shift. ~3kb compiled.
 */

export function ChainOrbits({ className = '' }: { className?: string }) {
  return (
    <div
      className={`absolute inset-0 pointer-events-none overflow-hidden ${className}`}
      aria-hidden="true"
    >
      {/* Layer 1 — taifoon.io's 3D-tilted infinite grid */}
      <div className="tf-grid absolute inset-[-10%]" />

      {/* Layer 2 — two concentric orbital traces, square nodes */}
      <div className="absolute left-1/2 top-1/2 -translate-x-1/2 -translate-y-1/2">
        {/* outer ring — azure, 90s, counter-clockwise */}
        <div
          className="tf-orbital-trace relative"
          style={{
            width: 'min(80vh, 720px)',
            height: 'min(80vh, 720px)',
          }}
        >
          <RingSquare
            stroke="rgba(61, 165, 255, 0.10)"
            nodeColor="#3DA5FF"
            nodeAngle={42}
            label="BASE · ARB · ETH · OP · POL"
          />
        </div>

        {/* inner ring — azure→mint, 70s, clockwise */}
        <div
          className="tf-orbital-trace-2 absolute inset-0 grid place-items-center"
          aria-hidden="true"
        >
          <div
            className="relative"
            style={{
              width: 'min(56vh, 480px)',
              height: 'min(56vh, 480px)',
            }}
          >
            <RingSquare
              stroke="rgba(20, 241, 149, 0.12)"
              nodeColor="#14F195"
              nodeAngle={210}
              label="SOL · DEVNET"
              mint
            />
          </div>
        </div>
      </div>

      {/* Layer 3 — central singularity (one breathing square node) */}
      <div className="absolute left-1/2 top-1/2 -translate-x-1/2 -translate-y-1/2">
        <div
          className="tf-breathe"
          style={{
            width: 8,
            height: 8,
            background: '#3DA5FF',
            boxShadow:
              '0 0 12px rgba(61, 165, 255, 0.6), 0 0 28px rgba(61, 165, 255, 0.25)',
          }}
        />
      </div>

      {/* Layer 4 — the connecting line (SOL ↔ root, pulses) */}
      <svg
        className="absolute left-1/2 top-1/2 -translate-x-1/2 -translate-y-1/2"
        width="min(56vh, 480px)"
        height="min(56vh, 480px)"
        viewBox="0 0 480 480"
        style={{ width: 'min(56vh, 480px)', height: 'min(56vh, 480px)' }}
      >
        <defs>
          <linearGradient id="tf-link" x1="0%" y1="0%" x2="100%" y2="100%">
            <stop offset="0%" stopColor="#3DA5FF" stopOpacity="0" />
            <stop offset="50%" stopColor="#14F195" stopOpacity="0.8" />
            <stop offset="100%" stopColor="#3DA5FF" stopOpacity="0" />
          </linearGradient>
        </defs>
        <line
          x1="240"
          y1="240"
          x2={240 + 240 * Math.cos((210 * Math.PI) / 180)}
          y2={240 + 240 * Math.sin((210 * Math.PI) / 180)}
          stroke="url(#tf-link)"
          strokeWidth="1"
          style={{ animation: 'tf-link-pulse 4s ease-in-out infinite' }}
        />
        <style>{`
          @keyframes tf-link-pulse {
            0%, 100% { opacity: 0.2; }
            50%      { opacity: 1; }
          }
        `}</style>
      </svg>
    </div>
  )
}

/**
 * RingSquare — outline ring + a single square node positioned at `nodeAngle`
 * (degrees, 0 = right, 90 = bottom). The orbital wrapper rotates the
 * whole ring infinitely; the node label is counter-rotated so it stays
 * upright in CSS via `transform: rotate(0)` on a child wrapper.
 */
function RingSquare({
  stroke,
  nodeColor,
  nodeAngle,
  label,
  mint,
}: {
  stroke: string
  nodeColor: string
  nodeAngle: number
  label: string
  mint?: boolean
}) {
  return (
    <div className="relative w-full h-full">
      {/* Ring — 1px circle */}
      <div
        className="absolute inset-0 rounded-full"
        style={{
          border: `1px solid ${stroke}`,
        }}
      />

      {/* Tiny tick marks at the cardinals (matches taifoon.io's vocabulary) */}
      {[0, 90, 180, 270].map((deg) => (
        <div
          key={deg}
          className="absolute left-1/2 top-1/2"
          style={{
            transform: `translate(-50%, -50%) rotate(${deg}deg) translateY(-50%)`,
          }}
        >
          <div
            style={{
              width: 4,
              height: 1,
              background: 'rgba(230, 240, 247, 0.18)',
              transform: 'translateY(-1px)',
            }}
          />
        </div>
      ))}

      {/* Square node at `nodeAngle` */}
      <div
        className="absolute left-1/2 top-1/2"
        style={{
          transform: `translate(-50%, -50%) rotate(${nodeAngle}deg) translateY(-50%)`,
        }}
      >
        <div
          style={{
            width: mint ? 8 : 6,
            height: mint ? 8 : 6,
            background: nodeColor,
            boxShadow: mint
              ? `0 0 14px ${nodeColor}, 0 0 30px ${nodeColor}55`
              : `0 0 8px ${nodeColor}88`,
            transform: `rotate(-${nodeAngle}deg)`, // keep upright
          }}
        />
        <div
          style={{
            position: 'absolute',
            top: mint ? 16 : 12,
            left: '50%',
            transform: `translateX(-50%) rotate(-${nodeAngle}deg)`,
            fontFamily: 'JetBrains Mono, monospace',
            fontSize: 9,
            letterSpacing: '0.24em',
            textTransform: 'uppercase',
            color: nodeColor,
            opacity: 0.6,
            whiteSpace: 'nowrap',
          }}
        >
          {label}
        </div>
      </div>
    </div>
  )
}
