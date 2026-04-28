import { ReactNode } from 'react'
import { cn } from './cn'

interface StepperProps {
  steps: { label: string; description?: string }[]
  current: number
  className?: string
}

/**
 * Stepper styled after taifoon.io's vertical phase timeline:
 *  - small mono "PHASE 0X — STEP" labels in azure
 *  - tiny square nodes connected by a hairline
 */
export function Stepper({ steps, current, className }: StepperProps) {
  return (
    <ol className={cn('flex items-stretch w-full', className)}>
      {steps.map((s, i) => {
        const isComplete = i < current
        const isActive = i === current
        const isLast = i === steps.length - 1
        return (
          <li key={s.label} className="flex-1 min-w-0">
            <div className="flex items-center">
              <div
                className={cn(
                  'w-2.5 h-2.5 shrink-0 rounded-[1px] transition-colors',
                  isComplete && 'bg-[var(--brand-blue)]',
                  isActive && 'bg-[var(--brand-blue)] outline outline-[3px] outline-[var(--brand-blue)]/20 outline-offset-1',
                  !isComplete && !isActive && 'bg-[var(--border-default)]',
                )}
              />
              {!isLast && (
                <div
                  className={cn(
                    'flex-1 h-px mx-2',
                    i < current ? 'bg-[var(--brand-blue)]' : 'bg-[var(--border-default)]',
                  )}
                />
              )}
            </div>
            <div className="mt-3 pr-2">
              <div
                className={cn(
                  'tf-phase',
                  isActive ? 'text-[var(--brand-blue)]' : 'text-[var(--text-tertiary)]',
                )}
              >
                PHASE {String(i + 1).padStart(2, '0')} — {s.label.toUpperCase()}
              </div>
              {s.description && (
                <div
                  className={cn(
                    'mt-2 text-xs leading-relaxed',
                    isActive ? 'text-[var(--text-primary)]' : 'text-[var(--text-tertiary)]',
                  )}
                >
                  {s.description}
                </div>
              )}
            </div>
          </li>
        )
      })}
    </ol>
  )
}

export function StepBody({ children }: { children: ReactNode }) {
  return <div className="mt-10">{children}</div>
}
