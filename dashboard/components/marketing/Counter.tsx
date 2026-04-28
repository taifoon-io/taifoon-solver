'use client'
import { useEffect, useRef, useState } from 'react'

/**
 * Counter — smoothly animates a number from a base value upward.
 * Used in the hero to make stat tiles feel alive — e.g. "$ filled today"
 * starts at a believable base and climbs as you watch.
 *
 * Animation runs only after mount (so SSR is stable). Step + interval
 * are tuned so the counter never feels frantic.
 */

export function Counter({
  base,
  step = 1,
  interval = 2200,
  prefix = '',
  format = 'int',
  className = '',
}: {
  base: number
  step?: number
  interval?: number
  prefix?: string
  /** 'int' = no decimals, 'usd' = $X,XXX.XX, 'compact' = 31, 38+, etc. */
  format?: 'int' | 'usd' | 'compact'
  className?: string
}) {
  const [value, setValue] = useState(base)
  const ref = useRef(base)

  useEffect(() => {
    const id = setInterval(() => {
      const inc = step + Math.random() * step
      ref.current += inc
      setValue(ref.current)
    }, interval)
    return () => clearInterval(id)
  }, [step, interval])

  const display = (() => {
    if (format === 'usd') {
      return value.toLocaleString('en-US', {
        minimumFractionDigits: 2,
        maximumFractionDigits: 2,
      })
    }
    if (format === 'compact') {
      return Math.round(value).toString()
    }
    return Math.round(value).toLocaleString('en-US')
  })()

  return (
    <span className={`tabular-nums ${className}`}>
      {prefix}
      {display}
    </span>
  )
}
