'use client'
import Link from 'next/link'
import { ReactNode, ButtonHTMLAttributes } from 'react'
import { cn } from './cn'

type Variant = 'primary' | 'secondary' | 'ghost' | 'mint'
type Size = 'sm' | 'md' | 'lg'

/**
 * Buttons:
 *  - JetBrains Mono labels, sentence-case, normal tracking
 *  - Outlined / hairline borders, tight radii
 *
 * Variant guide:
 *  - primary    → outlined azure, page-level CTA
 *  - secondary  → outlined neutral, supporting action
 *  - ghost      → no border, link-like
 *  - mint       → solana-mint accent, solver-specific moments only
 *
 * Voice: prefer sentence-case labels ("Spin up a solver") over the
 * ALL-CAPS terminal-cosplay register. Avoid prefix glyphs (`>`) and
 * trailing arrows (→) inside button copy — let the button itself be
 * the affordance.
 */

const variants: Record<Variant, string> = {
  primary:
    'border border-[var(--brand-blue)]/60 text-[var(--brand-blue)] hover:bg-[var(--brand-blue)]/10 hover:border-[var(--brand-blue)]',
  secondary:
    'border border-[var(--border-default)] text-[var(--text-primary)] hover:border-[var(--border-strong)] hover:bg-[var(--bg-elevated)]',
  ghost:
    'text-[var(--text-secondary)] hover:text-[var(--text-primary)]',
  mint:
    'border border-[var(--solana-mint)]/60 text-[var(--solana-mint)] hover:bg-[var(--solana-mint)]/10 hover:border-[var(--solana-mint)]',
}

const sizes: Record<Size, string> = {
  sm: 'h-8 px-3 text-[12px]',
  md: 'h-10 px-4 text-[13px]',
  lg: 'h-12 px-6 text-[14px]',
}

const base =
  'inline-flex items-center justify-center gap-2 font-mono tracking-[0.01em] ' +
  'rounded-[var(--r-sm)] transition-all duration-[var(--dur-base)] ease-[var(--ease-out)] ' +
  'disabled:opacity-40 disabled:cursor-not-allowed whitespace-nowrap'

interface CommonProps {
  variant?: Variant
  size?: Size
  className?: string
  children: ReactNode
  leadingIcon?: ReactNode
  trailingIcon?: ReactNode
}

type ButtonProps = CommonProps &
  Omit<ButtonHTMLAttributes<HTMLButtonElement>, 'children'> & { href?: undefined }

type LinkButtonProps = CommonProps & {
  href: string
  external?: boolean
}

export function Button(props: ButtonProps | LinkButtonProps) {
  const {
    variant = 'primary',
    size = 'md',
    className,
    children,
    leadingIcon,
    trailingIcon,
  } = props

  const classes = cn(base, variants[variant], sizes[size], className)
  const inner = (
    <>
      {leadingIcon}
      <span>{children}</span>
      {trailingIcon}
    </>
  )

  if ('href' in props && props.href) {
    if (props.external) {
      return (
        <a href={props.href} target="_blank" rel="noreferrer" className={classes}>
          {inner}
        </a>
      )
    }
    return (
      <Link href={props.href} className={classes}>
        {inner}
      </Link>
    )
  }

  const buttonProps = props as ButtonProps
  const {
    variant: _variant,
    size: _size,
    className: _className,
    leadingIcon: _leadingIcon,
    trailingIcon: _trailingIcon,
    children: _children,
    ...rest
  } = buttonProps
  void _variant
  void _size
  void _className
  void _leadingIcon
  void _trailingIcon
  void _children

  return (
    <button className={classes} {...rest}>
      {inner}
    </button>
  )
}
