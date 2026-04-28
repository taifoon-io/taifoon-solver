# solver.taifoon.dev — Design System

A documented set of tokens, primitives, and patterns powering the
solver.taifoon.dev marketing site, portal, and live monitor.

solver.taifoon.dev is positioned as a **sibling product** to taifoon.io —
sharper edges, more electric palette, Solana-friendly. The cyan from the
parent brand is retained as a primary; a Solana-violet co-primary signals
"cross-chain native, Solana-native."

---

## Audit (extend mode — initial baseline)

### Summary
Components reviewed: 8 primitives, 4 page templates, 31-protocol palette.
Issues found before this pass: hardcoded colors throughout `page.tsx`,
no shared component library, single-route dashboard.
Score after this pass: **9/10** (all colors tokenized, primitives
extracted, three-route product surface live).

### Token coverage
| Category    | Defined | Notes                                                 |
|-------------|---------|-------------------------------------------------------|
| Colors      | 28      | 4 surfaces, 3 borders, 4 text, 5 brand, 4 semantic, 9 protocol |
| Typography  | 9       | xs (11px) → 4xl (80px) — Inter + JetBrains Mono       |
| Spacing     | 9       | 4px base — sp-1 through sp-24                          |
| Radii       | 5       | sm 4 / md 8 / lg 12 / xl 16 / pill                     |
| Shadows     | 5       | card / raised + 3 brand glows                          |
| Motion      | 4       | fast 120ms / base 200ms / slow 400ms / out ease        |

### Component completeness
| Component | States | Variants | Docs | Score |
|-----------|--------|----------|------|-------|
| Button    | default / hover / active / disabled / focus | primary / secondary / ghost / glow + 3 sizes | this file | 9/10 |
| Card      | default / glow border             | none / sm / md / lg padding             | this file | 8/10 |
| Badge     | default / pulsed dot              | neutral / success / warning / danger / info / violet | this file | 9/10 |
| StatTile  | default / delta indicator         | 6 tones                                  | this file | 9/10 |
| CodeBlock | default / copied                  | optional `noCopy`, optional lang label   | this file | 8/10 |
| Stepper   | pending / active / complete       | one variant — 3+ steps                   | this file | 8/10 |
| NavBar    | default / active link             | sticky, blurred                          | this file | 8/10 |
| Footer    | default                           | one variant                              | this file | 7/10 |

---

## Tokens

### Colors

**Surfaces**
| Token              | Value     | Use                          |
|--------------------|-----------|------------------------------|
| `--bg-base`        | `#050507` | Page background              |
| `--bg-elevated`    | `#0B0B10` | Card, modal                  |
| `--bg-raised`      | `#14141C` | Inner card, input            |
| `--bg-overlay`     | `#1C1C26` | Dropdown, tooltip            |

**Brand**
| Token              | Value     | Use                          |
|--------------------|-----------|------------------------------|
| `--brand-cyan`     | `#00D9FF` | Primary action, focus, info  |
| `--brand-violet`   | `#9945FF` | Solana co-primary, accents   |
| `--brand-glow`     | `#14F195` | Solana mint, sparingly       |

**Semantic**
| Token              | Value     | Use                          |
|--------------------|-----------|------------------------------|
| `--success`        | `#00FF88` | Profit, confirmed, healthy   |
| `--warning`        | `#FFB800` | Dry-run, caution             |
| `--danger`         | `#FF3366` | Failed, reverted, loss       |
| `--info`           | `#00D9FF` | Informational badges         |

**Protocol palette (used in pills, rows, marquee)**
across (cyan), debridge (orange), mayan (violet), lifi (yellow),
orbiter (gray), stargate (light cyan), t3rn (mint), wormhole (coral),
cctp (Circle blue), hop, connext, synapse, celer, axelar,
hyperlane, layerzero, socket, squid, rango, symbiosis, meson,
allbridge, router, ccip.

### Typography
- **Sans**: Inter (display + body) — `--font-sans`
- **Mono**: JetBrains Mono (numbers, code, addresses) — `--font-mono`

Scale: `--fs-xs` 11px → `--fs-4xl` 80px. Headlines clamp via `clamp()` for
fluid sizing on the landing hero.

### Motion
All transitions use `--ease-out: cubic-bezier(0.16, 1, 0.3, 1)`. Durations
`--dur-fast` 120ms (hover), `--dur-base` 200ms (state change), `--dur-slow`
400ms (page-level animation).

---

## Components

### Button
The single interactive primitive. Used for all CTAs and link-buttons.

| Prop          | Type                                     | Default     |
|---------------|------------------------------------------|-------------|
| `variant`     | `primary \| secondary \| ghost \| glow`  | `primary`   |
| `size`        | `sm \| md \| lg`                         | `md`        |
| `href`        | `string`                                 | —           |
| `external`    | `boolean`                                | `false`     |
| `leadingIcon` | `ReactNode`                              | —           |
| `trailingIcon`| `ReactNode`                              | —           |

**Variants**
| Variant     | Use                                          |
|-------------|----------------------------------------------|
| `primary`   | Main CTA — cyan fill, dark text              |
| `secondary` | Supporting action — bordered, neutral        |
| `ghost`     | Tertiary — transparent, subtle hover         |
| `glow`      | Attention — cyan→violet gradient + shadow    |

**States**: default · hover (brighter) · active (dim) · disabled (50% opacity, no pointer events) · focus-visible (cyan ring).

**Accessibility**
- Role: `button` or implicit anchor (when `href` is present).
- Keyboard: standard — Tab to focus, Enter/Space to activate.
- Focus ring: 2px solid `--brand-cyan` with 2px offset.

### Card / CardHeader
Generic surface with optional gradient-border emphasis. Padding tokens
`none / sm / md / lg`. `glow` prop adds a cyan↔violet 1px gradient border —
reserved for the "primary tile" on a screen.

### Badge
Inline status indicator. Tones: neutral · success · warning · danger · info · violet. Optional `dot` (filled circle) and `pulse` (animated). Used for solver status, lambda stages, "LIVE" indicators.

### StatTile
Numeric KPI tile. Used in the landing hero counter strip, portal fleet
summary, and the live-monitor stats row. Tones colorize the number; an
optional `unit` shows e.g. "ms" subscript-style; an optional `delta` shows
↑/↓ percent change.

### CodeBlock
macOS-traffic-light-style terminal block. Includes a "copy" affordance
(disabled with `noCopy`) and an optional `lang` label. Used in landing's
"How it works" and the onboarding launch step.

### Stepper / StepBody
Inline step indicator with completed (filled) / active (outlined) / pending
(muted) states. Used in `/onboard` to communicate the four-step wizard.

### NavBar
Sticky, blur-backed header. Logo left, route links center (active link
shows in cyan with a raised background), GitHub + "Spin up solver" CTA
right. Active state derived from `usePathname()`.

### Footer
4-column footer: brand blurb · product links · protocols list · hackathon
links. Bottom strip: license + version metadata.

---

## Patterns

### Live indicator
A 2px green dot wrapped in `pulse-live` keyframes, paired with a "LIVE"
text label or `<Badge tone="success" dot pulse>`. Used wherever a solver,
stream, or pod is actively connected.

### Protocol pill
Inline color-coded chip — `inline-flex` of dot + name. Color comes from
`protocolColors[protocol-key]` in `lib/tokens.ts`. Used on:
the landing marquee, solver cards in `/portal`, intent rows in
`/portal/[solverId]`, and the `/onboard` protocol picker.

### Lambda lifecycle bar
Six-segment progress bar showing intent stage flow:
`detected → profitability_check → calldata_build → estimate_gate → broadcast → confirmed`.
Terminal stages (skipped, failed, reverted, dry_run) collapse the bar and
show a single colored badge.

### Mesh + grid background
The landing hero overlays two pseudo-elements: `.mesh-bg` (radial
gradients in cyan, violet, and mint) and `.grid-bg` (low-opacity 48px
grid). Both have `pointer-events: none` and live in `globals.css`.

---

## Routes

| Route                | Purpose                                              |
|----------------------|------------------------------------------------------|
| `/`                  | Landing — Colosseum / cross-chain marketing          |
| `/portal`            | Multi-solver fleet view + onboarding launcher        |
| `/portal/[solverId]` | Live monitor (existing dashboard, parameterized)     |
| `/onboard`           | 4-step solver onboarding wizard                      |
| `/docs`              | Stub linking to repo                                 |
| `/api/solver/*`      | Pre-existing SSE proxy (unchanged)                   |

---

## Voice & tone

Following BRAND.md:
- **Professional** — money-making infrastructure, not a toy.
- **Transparent** — every number visible, every stage logged.
- **Confident** — "the fastest way," not "a fast way."
- **Precise** — show decimals, gas costs, latency in ms.

Avoid: hyperbole that can't be backed by the dashboard. Lean into
specifics ("31 protocols, 38+ chains, 127ms median latency") instead of
generic claims.

---

## Open questions / next iterations

- **Logos** — current marquee uses text pills; eventually swap in real
  protocol SVG logos as they're collected.
- **Theming** — only dark mode today. A light mode could come later for
  embedded/iframe contexts.
- **Empty states** — `/portal` always shows the demo fleet. Production
  will need an empty-fleet zero-state pointing at `/onboard`.
- **Mobile portal** — the live monitor is dense; consider a mobile-only
  collapsed layout with priority cards.
- **Token sync** — `lib/tokens.ts` and `globals.css` are duplicated. A
  next pass could generate one from the other.
