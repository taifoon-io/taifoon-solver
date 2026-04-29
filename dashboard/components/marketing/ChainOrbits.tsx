'use client'
/**
 * ChainOrbits — re-envisioned hero geometry, v3.
 *
 * Composition (back to front, single 3D scene, single light, one camera):
 *
 *   1. tf-grid           80px square grid, 1.7° X-tilt, 180s drift (overhead plane)
 *   2. tf-floor          Perspective grid floor — 55° X-tilt, vanishing-point
 *                        recession, drifts toward camera over 12s
 *   3. orbital ring A    Tilted ~25° around X, azure stroke, 90s spin, square node
 *   4. orbital ring B    Tilted ~−15° around X with a small Y twist, mint stroke,
 *                        70s reverse spin, mint square node — the "Solana plane"
 *   5. tf-gyro           CSS-3D wireframe gyroscope at the singularity:
 *                        three perpendicular hairline rings co-rotating over 60s
 *                        with a mint core breathing on 6s
 *   6. link line         A faint mint line connecting Solana node → singularity
 *
 * Color palette is locked to four ambient values — anything outside this
 * set is a defect:
 *   - rgba(230, 240, 247, 0.024)  white-overhead
 *   - rgba( 61, 165, 255, 0.05)   azure-floor
 *   - rgba( 61, 165, 255, 0.55)   azure-stroke
 *   - rgba( 20, 241, 149, 0.65)   mint-stroke (+ #14F195 solid for the core)
 *
 * No JS animation state. No layout shift. No external libraries.
 */

export function ChainOrbits({ className = '' }: { className?: string }) {
  return (
    <div
      className={`absolute inset-0 pointer-events-none overflow-hidden ${className}`}
      aria-hidden="true"
    >
      {/* ── Layer 1 — overhead drift grid ─────────────────────────── */}
      <div className="tf-grid absolute inset-[-10%]" />

      {/* ── Layer 2 — perspective floor (Tron-style infinite recession) ── */}
      <div className="tf-floor-wrap absolute inset-0">
        <div className="tf-floor" />
        {/* Horizon fade — no visible far edge */}
        <div
          className="absolute inset-0 pointer-events-none"
          style={{
            background:
              'linear-gradient(180deg, rgba(0,0,0,1) 0%, rgba(0,0,0,1) 36%, rgba(0,0,0,0.6) 50%, rgba(0,0,0,0) 70%)',
          }}
        />
      </div>

      {/* ── Layers 3 & 4 — two 3D-tilted orbital rings ───────────── */}
      <div className="absolute left-1/2 top-1/2 -translate-x-1/2 -translate-y-1/2">
        {/* Outer azure ring — tilted ~25° around X */}
        <div
          style={{
            width: 'min(78vh, 700px)',
            height: 'min(78vh, 700px)',
            transformStyle: 'preserve-3d',
            transform: 'rotateX(25deg)',
          }}
        >
          <div className="tf-orbital-trace relative w-full h-full">
            <RingNode
              stroke="rgba(61, 165, 255, 0.32)"
              nodeColor="#3DA5FF"
              nodeAngle={42}
              label="EVM PLANE"
            />
          </div>
        </div>

        {/* Inner mint ring — tilted ~−15° around X with a small Y twist */}
        <div
          className="absolute inset-0 grid place-items-center"
          style={{ transformStyle: 'preserve-3d' }}
        >
          <div
            style={{
              width: 'min(54vh, 460px)',
              height: 'min(54vh, 460px)',
              transformStyle: 'preserve-3d',
              transform: 'rotateX(-15deg) rotateY(8deg)',
            }}
          >
            <div className="tf-orbital-trace-2 relative w-full h-full">
              <RingNode
                stroke="rgba(20, 241, 149, 0.42)"
                nodeColor="#14F195"
                nodeAngle={210}
                label="SVM PLANE"
                solana
              />
            </div>
          </div>
        </div>
      </div>

      {/* ── Layer 5 — wireframe gyroscope at the singularity ──────── */}
      <div className="absolute left-1/2 top-1/2 -translate-x-1/2 -translate-y-1/2">
        <div
          style={{
            perspective: 600,
            perspectiveOrigin: '50% 50%',
          }}
        >
          <div className="tf-gyro">
            <div className="tf-gyro-ring tf-gyro-ring--z" />
            <div className="tf-gyro-ring tf-gyro-ring--x" />
            <div className="tf-gyro-ring tf-gyro-ring--y" />
            <div className="tf-gyro-core" />
          </div>
        </div>
      </div>

      {/* ── Layer 6 — link line, singularity ↔ Solana node ────────── */}
      <svg
        className="absolute left-1/2 top-1/2 -translate-x-1/2 -translate-y-1/2"
        width="min(54vh, 460px)"
        height="min(54vh, 460px)"
        viewBox="0 0 460 460"
        style={{ width: 'min(54vh, 460px)', height: 'min(54vh, 460px)' }}
      >
        <defs>
          <linearGradient id="tf-link-grad" x1="0%" y1="0%" x2="100%" y2="100%">
            <stop offset="0%" stopColor="#3DA5FF" stopOpacity="0" />
            <stop offset="50%" stopColor="#14F195" stopOpacity="0.65" />
            <stop offset="100%" stopColor="#3DA5FF" stopOpacity="0" />
          </linearGradient>
        </defs>
        <line
          x1="230"
          y1="230"
          x2={230 + 230 * Math.cos((210 * Math.PI) / 180)}
          y2={230 + 230 * Math.sin((210 * Math.PI) / 180)}
          stroke="url(#tf-link-grad)"
          strokeWidth="1"
          style={{ animation: 'tf-link-pulse 4s ease-in-out infinite' }}
        />
        <style>{`
          @keyframes tf-link-pulse {
            0%, 100% { opacity: 0.18; }
            50%      { opacity: 1; }
          }
        `}</style>
      </svg>
    </div>
  )
}

/**
 * RingNode — outline ring + cardinal tick marks + a single square node
 * positioned at `nodeAngle` (degrees, 0 = right, 90 = bottom).
 *
 * Lives inside a 3D-tilted parent. The ring inherits the parent's
 * inclination automatically. The square node is counter-rotated only
 * around Z so its label remains roughly upright.
 */
function RingNode({
  stroke,
  nodeColor,
  nodeAngle,
  label,
  solana,
}: {
  stroke: string
  nodeColor: string
  nodeAngle: number
  label: string
  solana?: boolean
}) {
  return (
    <div className="relative w-full h-full">
      {/* Ring — 1px circle */}
      <div
        className="absolute inset-0 rounded-full"
        style={{ border: `1px solid ${stroke}` }}
      />

      {/* Cardinal tick marks at 0/90/180/270 — taifoon.io vocabulary */}
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
              width: 5,
              height: 1,
              background: 'rgba(230, 240, 247, 0.18)',
              transform: 'translateY(-1px)',
            }}
          />
        </div>
      ))}

      {/* Square node */}
      <div
        className="absolute left-1/2 top-1/2"
        style={{
          transform: `translate(-50%, -50%) rotate(${nodeAngle}deg) translateY(-50%)`,
        }}
      >
        <div
          style={{
            width: solana ? 9 : 7,
            height: solana ? 9 : 7,
            background: nodeColor,
            boxShadow: solana
              ? `0 0 14px ${nodeColor}, 0 0 32px ${nodeColor}55`
              : `0 0 8px ${nodeColor}88`,
            transform: `rotate(-${nodeAngle}deg)`,
          }}
        />
        <div
          style={{
            position: 'absolute',
            top: solana ? 16 : 12,
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
