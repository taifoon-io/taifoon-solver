'use client'
import Link from 'next/link'
import { ReactNode, ButtonHTMLAttributes } from 'react'
import { cn } from './cn'

type Variant = 'primary' | 'secondary' | 'ghost' | 'glow'
type Size = 'sm' | 'md' | 'lg'

const variants: Record<Variant, string> = {
  primary:
    'bg-[var(--brand-cyan)] text-black hover:bg-[#33E1FF] active:bg-[var(--brand-cyan-dim)] shadow-[var(--glow-cyan)]',
  secondary:
    'bg-[var(--bg-raised)] text-[var(--text-primary)] border border-[var(--border-default)] hover:border-[var(--border-strong)] hover:bg-[var(--bg-overlay)]',
  ghost:
    'bg-transparent text-[var(--text-secondary)] hover:text-[var(--text-primary)] hover:bg-[var(--bg-elevated)]',
  glow:
    'bg-gradient-to-r from-[var(--brand-cyan)] to-[var(--brand-violet)] text-black font-bold hover:brightness-110 shadow-[var(--glow-violet)]',
}

const sizes: Record<Size, string> = {
  sm: 'h-7 px-3 text-[11px]',
  md: 'h-9 px-4 text-[13px]',
  lg: 'h-11 px-6 text-[15px]',
}

const base =
  'inline-flex items-center justify-center gap-2 rounded-[var(--r-md)] font-semibold tracking-wide ' +
  'transition-all duration-[var(--dur-base)] ease-[var(--ease-out)] disabled:opacity-50 disabled:cursor-not-allowed ' +
  'focus-visible:outline-2 focus-visible:outline-[var(--brand-cyan)] focus-visible:outline-offset-2 whitespace-nowrap'

interface CommonProps {
  variant?: Variant
  size?: Size
  className?: string
  children: ReactNode
  leadingIcon?: ReactNode
  trailingIcon?: ReactNode
}

type ButtonProps = CommonProps & Omit<ButtonHTMLAttributes<HTMLButtonElement>, 'children'> & {
  href?: undefined
}
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
  // Strip styling props so we can spread the rest onto the button element.
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
  void _variant; void _size; void _className; void _leadingIcon; void _trailingIcon; void _children
  return (
    <button className={classes} {...rest}>
      {inner}
    </button>
  )
}
