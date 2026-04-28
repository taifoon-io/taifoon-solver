'use client'
import { useState } from 'react'
import { cn } from './cn'

/**
 * Snippet — the canonical code-block. Replaces ad-hoc CodeBlock usage.
 *
 * Three modes, all equalized to the same width / prompt rhythm / copy
 * affordance so they feel like one family across the site:
 *
 *   compact   — single-line install command, no chrome bar
 *   default   — fenced block with mono header (lang + [ COPY ])
 *   tabbed    — multi-step sequence — tabs for INSTALL / ONBOARD / RUN
 *
 * Visual contract:
 *   - 1px border, hairline divider under header
 *   - $ prompt prefix (azure) before each command, except in compact mode
 *   - JetBrains Mono everywhere, 12px body
 *   - Width is set by parent — Snippet always fills its container
 */

interface SnippetTab {
  label: string
  lang?: string
  code: string
}

type SnippetProps =
  | {
      variant?: 'compact'
      code: string
      lang?: never
      tabs?: never
      className?: string
      noCopy?: boolean
    }
  | {
      variant?: 'default'
      code: string
      lang?: string
      tabs?: never
      className?: string
      noCopy?: boolean
    }
  | {
      variant: 'tabbed'
      tabs: SnippetTab[]
      code?: never
      lang?: never
      className?: string
      noCopy?: boolean
    }

export function Snippet(props: SnippetProps) {
  const variant = props.variant ?? 'default'
  if (variant === 'compact') return <CompactSnippet {...(props as SnippetProps & { variant: 'compact' })} />
  if (variant === 'tabbed') return <TabbedSnippet {...(props as SnippetProps & { variant: 'tabbed' })} />
  return <DefaultSnippet {...(props as SnippetProps & { variant: 'default' })} />
}

// ── compact: one-liner ────────────────────────────────────────────────
function CompactSnippet({
  code,
  className,
  noCopy,
}: {
  code: string
  className?: string
  noCopy?: boolean
}) {
  return (
    <div
      className={cn(
        'flex items-center gap-3 h-10 px-3 rounded-[var(--r-sm)] border border-[var(--border-default)]',
        'bg-[var(--bg-elevated)] font-mono text-[12px] text-[var(--text-primary)]',
        className,
      )}
    >
      <span className="text-[var(--brand-blue)] shrink-0">$</span>
      <span className="truncate flex-1">{code}</span>
      {!noCopy && <CopyButton code={code} />}
    </div>
  )
}

// ── default: fenced block ─────────────────────────────────────────────
function DefaultSnippet({
  code,
  lang,
  className,
  noCopy,
}: {
  code: string
  lang?: string
  className?: string
  noCopy?: boolean
}) {
  return (
    <div
      className={cn(
        'rounded-[var(--r-sm)] border border-[var(--border-default)] bg-[var(--bg-elevated)] overflow-hidden',
        className,
      )}
    >
      <SnippetHeader lang={lang} code={code} noCopy={noCopy} />
      <Code code={code} />
    </div>
  )
}

// ── tabbed: multi-step ────────────────────────────────────────────────
function TabbedSnippet({
  tabs,
  className,
  noCopy,
}: {
  tabs: SnippetTab[]
  className?: string
  noCopy?: boolean
}) {
  const [active, setActive] = useState(0)
  const cur = tabs[active]
  return (
    <div
      className={cn(
        'rounded-[var(--r-sm)] border border-[var(--border-default)] bg-[var(--bg-elevated)] overflow-hidden',
        className,
      )}
    >
      <div className="flex items-center justify-between border-b border-[var(--border-subtle)] pl-1 pr-3">
        <div className="flex">
          {tabs.map((t, i) => (
            <button
              key={t.label}
              onClick={() => setActive(i)}
              className={cn(
                'h-9 px-3 font-mono text-[11px] tracking-[0.2em] uppercase transition-colors border-b-2',
                i === active
                  ? 'text-[var(--brand-blue)] border-[var(--brand-blue)]'
                  : 'text-[var(--text-tertiary)] border-transparent hover:text-[var(--text-primary)]',
              )}
            >
              0{i + 1} · {t.label}
            </button>
          ))}
        </div>
        {!noCopy && <CopyButton code={cur.code} />}
      </div>
      <Code code={cur.code} />
    </div>
  )
}

// ── shared bits ───────────────────────────────────────────────────────
function SnippetHeader({
  lang,
  code,
  noCopy,
}: {
  lang?: string
  code: string
  noCopy?: boolean
}) {
  return (
    <div className="flex items-center justify-between px-3 h-9 border-b border-[var(--border-subtle)]">
      <div className="flex items-center gap-3">
        <span className="text-[var(--brand-blue)] font-mono text-[11px]">$</span>
        {lang && (
          <span className="text-[10px] font-mono uppercase tracking-[0.22em] text-[var(--text-tertiary)]">
            {lang}
          </span>
        )}
      </div>
      {!noCopy && <CopyButton code={code} />}
    </div>
  )
}

function CopyButton({ code }: { code: string }) {
  const [copied, setCopied] = useState(false)
  return (
    <button
      onClick={async () => {
        try {
          await navigator.clipboard.writeText(code)
          setCopied(true)
          setTimeout(() => setCopied(false), 1400)
        } catch {
          /* no-op */
        }
      }}
      className="font-mono text-[10px] tracking-[0.22em] uppercase text-[var(--text-tertiary)] hover:text-[var(--brand-blue)] transition-colors"
    >
      {copied ? '[ COPIED ]' : '[ COPY ]'}
    </button>
  )
}

function Code({ code }: { code: string }) {
  // Render each line with a $ prompt prefix for shell rhythm. Skip the
  // prefix on lines that start with `#` (comments) so they render as
  // plain comments without the prompt.
  const lines = code.split('\n')
  return (
    <pre className="p-4 text-[12px] font-mono leading-[1.7] overflow-x-auto">
      <code>
        {lines.map((line, i) => {
          if (line.startsWith('#') || line.trim() === '') {
            return (
              <div key={i} className="text-[var(--text-tertiary)]">
                {line || ' '}
              </div>
            )
          }
          return (
            <div key={i} className="flex gap-3">
              <span className="text-[var(--brand-blue)] select-none">$</span>
              <span className="text-[var(--text-primary)]">{line}</span>
            </div>
          )
        })}
      </code>
    </pre>
  )
}
