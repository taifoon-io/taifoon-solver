'use client'
/**
 * ChainOrbits — unified 3D scene, v4.
 *
 * Six layers all parented to ONE perspective camera so they share a
 * single vanishing point. The "3D alignment" the brief asked for is
 * really one thing: every transform respects the same camera.
 *
 * Layers, back to front (low Z → high Z):
 *
 *   z = -300   tf-grid      overhead drift grid, pushed deep into the scene
 *   z = -100   tf-floor     Tron-style perspective floor flowing toward camera
 *   z = -150   particles*   azure/mint dust at varied Z, staggered breathing
 *   z = -40    orbit A      tilted +25° X, azure, 90s
 *   z =   0    orbit B      tilted -15° X, mint, 70s reverse — "Solana plane"
 *   z = +60    particles*   foreground particles, brighter
 *   z = +80    tf-gyro      wireframe gyroscope, brightest
 *
 * (* particles span -200 to +120, distributed across Z)
 *
 * Camera-tilt: perspective-origin 50% 32% — we're looking *down into*
 * the scene, not at it head-on. Single value, biggest perceptual win.
 *
 * Lighting cheat: a radial gradient overlay simulates a light source
 * at upper-right. The eye reads it as illumination direction.
 *
 * Palette is strictly locked to:
 *   - rgba(230,240,247,0.024 → 0.18)   white at low opacity
 *   - rgba(61,165,255, 0.05 → 0.7)     azure ramp
 *   - rgba(20,241,149, 0.42 → 0.9)     mint ramp + #14F195 solid
 */

const PARTICLES: ParticleSpec[] = [
  // Background dust (z < 0) — smaller, dimmer
  { x: 8,  y: 22, z: -180, size: 2, color: 'azure', dur: 9,  delay: 0 },
  { x: 18, y: 76, z: -160, size: 2, color: 'azure', dur: 7,  delay: 1.4 },
  { x: 84, y: 18, z: -200, size: 2, color: 'azure', dur: 11, delay: 2.7 },
  { x: 92, y: 64, z: -140, size: 3, color: 'azure', dur: 8,  delay: 0.6 },
  { x: 30, y: 40, z: -120, size: 2, color: 'mint',  dur: 12, delay: 3.4 },
  { x: 70, y: 82, z: -100, size: 2, color: 'azure', dur: 9,  delay: 4.1 },

  // Mid-plane (z ≈ 0)
  { x: 14, y: 50, z: -20,  size: 3, color: 'azure', dur: 6,  delay: 0.9 },
  { x: 86, y: 38, z: -10,  size: 3, color: 'azure', dur: 8,  delay: 2.2 },
  { x: 50, y: 12, z: 30,   size: 3, color: 'mint',  dur: 7,  delay: 1.6 },
  { x: 42, y: 88, z: 20,   size: 3, color: 'azure', dur: 10, delay: 3.0 },

  // Foreground (z > 0) — larger, brighter
  { x: 62, y: 28, z: 80,   size: 4, color: 'mint',  dur: 5,  delay: 0.3 },
  { x: 22, y: 70, z: 100,  size: 4, color: 'azure', dur: 6,  delay: 2.0 },
  { x: 78, y: 56, z: 60,   size: 3, color: 'azure', dur: 8,  delay: 4.5 },
  { x: 36, y: 24, z: 110,  size: 3, color: 'azure', dur: 7,  delay: 1.1 },
  { x: 58, y: 78, z: 70,   size: 4, color: 'mint',  dur: 9,  delay: 3.7 },

  // A few far-off bright bits to suggest infinite depth
  { x: 6,  y: 60, z: -260, size: 2, color: 'azure', dur: 14, delay: 0 },
  { x: 96, y: 48, z: -240, size: 2, color: 'azure', dur: 13, delay: 5.0 },
  { x: 50, y: 50, z: -300, size: 2, color: 'mint',  dur: 16, delay: 7.0 },
]

interface ParticleSpec {
  x: number
  y: number
  z: number
  size: number
  color: 'azure' | 'mint'
  dur: number
  delay: number
}

export function ChainOrbits({ className = '' }: { className?: string }) {
  return (
    <div
      className={`absolute inset-0 pointer-events-none overflow-hidden ${className}`}
      aria-hidden="true"
    >
      {/* ── ONE perspective scene ──────────────────────────────────── */}
      <div
        className="absolute inset-0"
        style={{
          perspective: '1400px',
          perspectiveOrigin: '50% 32%',
          transformStyle: 'preserve-3d',
        }}
      >
        {/* Layer 1 — overhead drift grid, pushed deep ─────────────── */}
        <div
          className="tf-grid absolute inset-[-15%]"
          style={{
            transform: 'translateZ(-300px)',
            transformStyle: 'preserve-3d',
          }}
        />

        {/* Layer 2 — Tron-style floor at the back of mid-plane ────── */}
        <div className="absolute inset-0" style={{ transformStyle: 'preserve-3d' }}>
          <div className="tf-floor" style={{ transform: 'rotateX(58deg) translateZ(-100px)' }} />
          <div
            className="absolute inset-0 pointer-events-none"
            style={{
              background:
                'linear-gradient(180deg, rgba(0,0,0,1) 0%, rgba(0,0,0,1) 36%, rgba(0,0,0,0.6) 50%, rgba(0,0,0,0) 70%)',
            }}
          />
        </div>

        {/* Layer 3 — particle dust field ──────────────────────────── */}
        <div
          className="absolute inset-0"
          style={{ transformStyle: 'preserve-3d' }}
        >
          {PARTICLES.map((p, i) => (
            <Particle key={i} {...p} />
          ))}
        </div>

        {/* Layer 4 — outer azure orbit, behind center ─────────────── */}
        <div
          className="absolute left-1/2 top-1/2"
          style={{
            transform: 'translate(-50%, -50%) translateZ(-40px) rotateX(25deg)',
            transformStyle: 'preserve-3d',
          }}
        >
          <div
            className="tf-orbital-trace relative"
            style={{
              width: 'min(78vh, 700px)',
              height: 'min(78vh, 700px)',
            }}
          >
            <RingNode
              stroke="rgba(61, 165, 255, 0.32)"
              nodeColor="#3DA5FF"
              nodeAngle={42}
              label="EVM PLANE"
            />
          </div>
        </div>

        {/* Layer 5 — inner mint orbit, mid-plane ──────────────────── */}
        <div
          className="absolute left-1/2 top-1/2"
          style={{
            transform: 'translate(-50%, -50%) rotateX(-15deg) rotateY(8deg)',
            transformStyle: 'preserve-3d',
          }}
        >
          <div
            className="tf-orbital-trace-2 relative"
            style={{
              width: 'min(54vh, 460px)',
              height: 'min(54vh, 460px)',
            }}
          >
            <RingNode
              stroke="rgba(20, 241, 149, 0.42)"
              nodeColor="#14F195"
              nodeAngle={210}
              label="SVM PLANE"
              solana
            />
          </div>
        </div>

        {/* Layer 6 — wireframe gyroscope, foreground ──────────────── */}
        <div
          className="absolute left-1/2 top-1/2"
          style={{
            transform: 'translate(-50%, -50%) translateZ(80px)',
            transformStyle: 'preserve-3d',
          }}
        >
          <div className="tf-gyro">
            <div className="tf-gyro-ring tf-gyro-ring--z" />
            <div className="tf-gyro-ring tf-gyro-ring--x" />
            <div className="tf-gyro-ring tf-gyro-ring--y" />
            <div className="tf-gyro-core" />
          </div>
        </div>

        {/* Layer 7 — link line, singularity ↔ Solana node ─────────── */}
        <svg
          className="absolute left-1/2 top-1/2"
          width="min(54vh, 460px)"
          height="min(54vh, 460px)"
          viewBox="0 0 460 460"
          style={{
            width: 'min(54vh, 460px)',
            height: 'min(54vh, 460px)',
            transform: 'translate(-50%, -50%) translateZ(20px)',
          }}
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

      {/* Directional rim-light — implies a light source upper-right */}
      <div
        className="absolute inset-0 pointer-events-none"
        style={{
          background:
            'radial-gradient(ellipse 60% 50% at 82% 12%, rgba(61, 165, 255, 0.08) 0%, transparent 60%)',
        }}
      />
      {/* Cool fog from the lower-left — reinforces volume */}
      <div
        className="absolute inset-0 pointer-events-none"
        style={{
          background:
            'radial-gradient(ellipse 55% 50% at 12% 88%, rgba(20, 241, 149, 0.05) 0%, transparent 60%)',
        }}
      />
    </div>
  )
}

/**
 * Particle — a single 3D-positioned dust square. Lives inside the
 * scene's perspective context, so its translateZ creates real parallax
 * relative to other layers.
 */
function Particle({ x, y, z, size, color, dur, delay }: ParticleSpec) {
  const hex = color === 'mint' ? '#14F195' : '#3DA5FF'
  // Closer particles are brighter — depth cue.
  const baseOpacity = z > 50 ? 0.7 : z > 0 ? 0.5 : z > -100 ? 0.35 : 0.22
  return (
    <span
      style={{
        position: 'absolute',
        left: `${x}%`,
        top: `${y}%`,
        width: size,
        height: size,
        background: hex,
        transform: `translate3d(-50%, -50%, ${z}px)`,
        opacity: baseOpacity,
        boxShadow: `0 0 ${size * 3}px ${hex}55`,
        animation: `tf-particle-fade ${dur}s ease-in-out infinite`,
        animationDelay: `${delay}s`,
      }}
    />
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
