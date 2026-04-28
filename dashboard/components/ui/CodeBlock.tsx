'use client'
import { useState } from 'react'
import { cn } from './cn'

interface CodeBlockProps {
  code: string
  lang?: string
  className?: string
  /** Removes the copy affordance for purely-illustrative blocks. */
  noCopy?: boolean
}

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
        'relative group rounded-[var(--r-md)] border border-[var(--border-default)]',
        'bg-[var(--bg-raised)] overflow-hidden',
        className,
      )}
    >
      <div className="flex items-center justify-between px-3 py-2 border-b border-[var(--border-subtle)] bg-[var(--bg-elevated)]">
        <div className="flex items-center gap-1.5">
          <span className="w-2.5 h-2.5 rounded-full bg-[#FF5F56]/70" />
          <span className="w-2.5 h-2.5 rounded-full bg-[#FFBD2E]/70" />
          <span className="w-2.5 h-2.5 rounded-full bg-[#27C93F]/70" />
          {lang && (
            <span className="ml-2 text-[10px] uppercase tracking-wider text-[var(--text-tertiary)]">
              {lang}
            </span>
          )}
        </div>
        {!noCopy && (
          <button
            onClick={onCopy}
            className="text-[10px] font-mono uppercase tracking-wider text-[var(--text-tertiary)] hover:text-[var(--brand-cyan)] transition-colors"
          >
            {copied ? '✓ copied' : 'copy'}
          </button>
        )}
      </div>
      <pre className="p-4 text-xs font-mono leading-relaxed text-[var(--text-primary)] overflow-x-auto">
        <code>{code}</code>
      </pre>
    </div>
  )
}
