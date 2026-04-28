'use client'
import { useState } from 'react'
import { cn } from './cn'

interface CodeBlockProps {
  code: string
  lang?: string
  className?: string
  noCopy?: boolean
}

/**
 * Terminal-style code block. Aligned with taifoon.io's prompt aesthetic —
 * a JetBrains Mono header strip with a `$` prompt and language tag, plus
 * a copy affordance. No traffic lights (those felt out of place against
 * the parent brand's stricter terminal vibe).
 */
export function CodeBlock({ code, lang, className, noCopy }: CodeBlockProps) {
  const [copied, setCopied] = useState(false)

  const onCopy = async () => {
    try {
      await navigator.clipboard.writeText(code)
      setCopied(true)
      setTimeout(() => setCopied(false), 1400)
    } catch {
      /* no-op */
    }
  }

  return (
    <div
      className={cn(
        'relative rounded-[var(--r-sm)] border border-[var(--border-default)]',
        'bg-[var(--bg-elevated)] overflow-hidden',
        className,
      )}
    >
      <div className="flex items-center justify-between px-3 py-2 border-b border-[var(--border-subtle)]">
        <div className="flex items-center gap-3">
          <span className="text-[var(--brand-blue)] font-mono text-[11px]">$</span>
          {lang && (
            <span className="text-[10px] font-mono uppercase tracking-[0.2em] text-[var(--text-tertiary)]">
              {lang}
            </span>
          )}
        </div>
        {!noCopy && (
          <button
            onClick={onCopy}
            className="text-[10px] font-mono uppercase tracking-[0.2em] text-[var(--text-tertiary)] hover:text-[var(--brand-blue)] transition-colors"
          >
            {copied ? '[ COPIED ]' : '[ COPY ]'}
          </button>
        )}
      </div>
      <pre className="p-4 text-xs font-mono leading-relaxed text-[var(--text-primary)] overflow-x-auto">
        <code>{code}</code>
      </pre>
    </div>
  )
}
