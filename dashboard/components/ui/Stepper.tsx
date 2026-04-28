import { ReactNode } from 'react'
import { cn } from './cn'

interface StepperProps {
  steps: { label: string; description?: string }[]
  current: number // zero-based
  className?: string
}

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
                  'w-7 h-7 rounded-full flex items-center justify-center text-xs font-bold shrink-0 border',
                  isComplete && 'bg-[var(--brand-cyan)] text-black border-[var(--brand-cyan)]',
                  isActive && 'border-[var(--brand-cyan)] text-[var(--brand-cyan)]',
                  !isComplete && !isActive && 'border-[var(--border-default)] text-[var(--text-tertiary)]',
                )}
              >
                {isComplete ? '✓' : i + 1}
              </div>
              {!isLast && (
                <div
                  className={cn(
                    'flex-1 h-px mx-2',
                    i < current ? 'bg-[var(--brand-cyan)]' : 'bg-[var(--border-default)]',
                  )}
                />
              )}
            </div>
            <div className="mt-2 pr-2">
              <div
                className={cn(
                  'text-xs font-semibold',
                  isActive ? 'text-[var(--text-primary)]' : 'text-[var(--text-tertiary)]',
                )}
              >
                {s.label}
              </div>
              {s.description && (
                <div className="text-[10px] text-[var(--text-tertiary)] mt-0.5 line-clamp-1">
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
  return <div className="mt-8">{children}</div>
}
