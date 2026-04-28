/** Tiny classname joiner — avoids pulling in `clsx` for a 1-file utility. */
export function cn(...args: (string | false | null | undefined)[]) {
  return args.filter(Boolean).join(' ')
}
