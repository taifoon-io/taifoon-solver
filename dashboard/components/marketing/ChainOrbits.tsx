'use client'
/**
 * ChainOrbits — calmed, v5.
 *
 * Three layers, one perspective camera. The previous version stacked
 * seven motion layers (drift grid, Tron floor, three particle fields,
 * two orbital rings, gyroscope, link line, two rim-light radials,
 * vignette) which competed for attention with the live volume firehose
 * to its right. Real infra pages are calm.
 *
 * Layers, back to front:
 *
 *   z = -100   tf-floor     Tron-style perspective floor flowing toward camera
 *   z =   0    orbit        single tilted ring, azure, 90s rotation
 *   z = +80    tf-gyro      wireframe gyroscope core
 *
 * Removed deliberately:
 *   - tf-grid overhead drift plane (was double-counting the floor)
 *   - 18 particles across three depth bands (theatrical, not informational)
 *   - second mint orbital ring (one ring is enough)
 *   - cross-orbital link-line SVG with pulse animation
 *   - upper-right azure rim-light + lower-left mint fog radials
 *   - vignette (now lives in `page.tsx` so the hero owns the legibility layer)
 */

export function ChainOrbits({ className = '' }: { className?: string }) {
  return (
    <div
      className={`absolute inset-0 pointer-events-none overflow-hidden ${className}`}
      aria-hidden="true"
    >
      <div
        className="absolute inset-0"
        style={{
          perspective: '1400px',
          perspectiveOrigin: '50% 32%',
          transformStyle: 'preserve-3d',
        }}
      >
        {/* Layer 1 — Tron floor */}
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

        {/* Layer 2 — single tilted azure orbit */}
        <div
          className="absolute left-1/2 top-1/2"
          style={{
            transform: 'translate(-50%, -50%) rotateX(20deg)',
            transformStyle: 'preserve-3d',
          }}
        >
          <div
            className="tf-orbital-trace relative"
            style={{
              width: 'min(64vh, 580px)',
              height: 'min(64vh, 580px)',
            }}
          >
            <div
              className="absolute inset-0 rounded-full"
              style={{ border: '1px solid rgba(61, 165, 255, 0.28)' }}
            />
            {/* cardinal ticks — minimal taifoon vocabulary */}
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
          </div>
        </div>

        {/* Layer 3 — wireframe gyroscope core */}
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
      </div>
    </div>
  )
}
