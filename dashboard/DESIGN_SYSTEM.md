# solver.taifoon.dev — Design System

A documented set of tokens, primitives, and patterns powering the
solver.taifoon.dev marketing site, portal, and live monitor.

solver.taifoon.dev is positioned as a **sibling product** to taifoon.io,
intentionally aligned with the parent brand:

- Pure-black canvas (`#000000`)
- Soft `#E6F0F7` ink at multiple opacities
- Single azure accent `#3DA5FF` borrowed from taifoon.io
- JetBrains Mono `[ TAG LABELS ]` with 3px tracking
- Inter weight-300 sentence-case display headlines
- Vertical phase timelines with `PHASE 0X — STEP` markers

Three intentional deviations from taifoon.io that signal "Solana-friendly
sibling, not a copy":

1. **Solana-mint accent** `#14F195` — used sparingly for live dots, P&L
   confirmations, and the **mint** Button variant on solver-specific CTAs.
2. **Wordmark** reads `TAIFOON / SPINNERS` in the nav — borrowing
   taifoon.io's term for solver pods.
3. **Solana-violet accent** `#9945FF` — used as a tertiary tone for
   protocol pills (Mayan Swift, Hop) and one Tag tone.

---

## Audit (after taifoon.io alignment pass)

### Summary
After this pass, every page uses tokens (zero hardcoded hex outside
`lib/tokens.ts` and protocol palette), the type system uses `tf-display`,
`tf-tag`, `tf-phase`, `tf-stat-prefix/value` utility classes, and the
voice + structure mirrors taifoon.io's `[ THE ENGINE ] [ THE ECONOMY ] [ THE COCKPIT ]`
narrative pattern.

Score: **9/10**. The remaining gap is genuine logo art for protocols
(currently text pills) and a real `/api/solvers` endpoint behind the
portal demo data.

### Token coverage

| Category    | Defined | Notes                                                      |
|-------------|---------|------------------------------------------------------------|
| Colors      | 22      | 4 surfaces, 3 borders, 4 text, 3 brand, 2 Solana, 4 semantic, 9 protocol |
| Typography  | 8       | xs (11px) → 4xl (96px) — Inter + JetBrains Mono            |
| Letter-spacing | 4    | tight (-0.02em), snug (0.01em), mono (0.05em), tag (0.25em) |
| Spacing     | 9       | 4px base                                                    |
| Radii       | 4       | 0/2/4/6 — kept tight to match taifoon.io's flat aesthetic   |
| Glows       | 2       | Azure + mint, used sparingly                                |
| Motion      | 4       | fast 120 / base 200 / slow 400 / out ease                   |

### Component completeness

| Component | States | Variants | Score |
|-----------|--------|----------|-------|
| Button    | default / hover / active / disabled / focus | primary / secondary / ghost / mint + 3 sizes | 9/10 |
| Card      | default / accent (top-line) | none / sm / md / lg padding | 9/10 |
| Badge     | default / pulsed dot | neutral / success / warning / danger / info / mint / violet | 9/10 |
| StatTile  | default / inline / stack | 7 tones | 9/10 |
| CodeBlock | default / copied | optional `noCopy`, optional lang | 9/10 |
| Stepper   | pending / active / complete | one variant — phase-style | 8/10 |
| NavBar    | default / active link | sticky, blurred, mono | 9/10 |
| Footer    | default | one variant | 8/10 |
| Tag       | default | 4 tones (blue / mint / violet / muted) | 9/10 |
| PhaseLabel| default | 3 tones (blue / mint / violet) | 9/10 |

---

## Tokens

### Colors

**Surfaces — pure-black canvas, taifoon.io-aligned**
| Token              | Value     | Use                          |
|--------------------|-----------|------------------------------|
| `--bg-base`        | `#000000` | Page background              |
| `--bg-elevated`    | `#050507` | Card                         |
| `--bg-raised`      | `#0A0A0F` | Inner card, input            |
| `--bg-overlay`     | `#12121A` | Dropdown, tooltip            |

**Text — soft blue-white at four opacities**
| Token              | Value                          |
|--------------------|--------------------------------|
| `--text-primary`   | `#E6F0F7`                      |
| `--text-secondary` | `rgba(230, 240, 247, 0.6)`     |
| `--text-tertiary`  | `rgba(230, 240, 247, 0.4)`     |
| `--text-disabled`  | `rgba(230, 240, 247, 0.2)`     |

**Brand**
| Token              | Value     | Use                                    |
|--------------------|-----------|----------------------------------------|
| `--brand-blue`     | `#3DA5FF` | The single primary accent — taifoon.io |
| `--solana-mint`    | `#14F195` | Solana flair — live dots, fills        |
| `--solana-violet`  | `#9945FF` | Solana flair — tertiary tags           |

**Semantic**
| Token       | Value     | Use                          |
|-------------|-----------|------------------------------|
| `--success` | `#14F195` | Confirmed fills, healthy P&L |
| `--warning` | `#FFB454` | Dry-run, caution             |
| `--danger`  | `#FF6B6B` | Failed, reverted             |
| `--info`    | `#3DA5FF` | Informational, links         |

### Typography

- **Sans**: Inter — `--font-sans`. Display headlines use weight 300 with
  tight tracking, sentence case (NOT all-caps).
- **Mono**: JetBrains Mono — `--font-mono`. Used for nav links, button
  labels, tag labels, stat values, addresses, log lines.

| Class              | Use                                                |
|--------------------|----------------------------------------------------|
| `tf-display`       | Inter 300, line-height 0.98, tight tracking        |
| `tf-gradient-silver` | White → silver gradient on display headlines     |
| `tf-gradient-solana` | Blue → mint gradient on solver-specific moments  |
| `tf-tag`           | Mono 12px, 0.25em tracking, azure — `[ THE ENGINE ]` |
| `tf-phase`         | Mono 11px, 0.25em tracking — `PHASE 01 — DETECT`   |
| `tf-stat-prefix`   | Mono 11px, faded — `real-time` `median`            |
| `tf-stat-value`    | Mono 26px, primary color — large numbers           |

### Motion

`--ease-out: cubic-bezier(0.16, 1, 0.3, 1)`. Durations 120ms / 200ms / 400ms.

---

## Components

### Button

The single interactive primitive. Outlined, mono, ALL-CAPS, tight radii.

| Prop          | Type                                           |
|---------------|------------------------------------------------|
| `variant`     | `primary \| secondary \| ghost \| mint`        |
| `size`        | `sm \| md \| lg`                               |
| `href`        | `string` — renders as `<Link>` or `<a>`        |
| `external`    | `boolean`                                      |

Variant guide:
- `primary` — outlined azure, page-level CTA
- `secondary` — outlined neutral, supporting action
- `ghost` — link-like, no border
- `mint` — outlined solana-mint, solver-specific moments only (e.g.
  "OPEN MY SPINNER" at the end of onboarding)

### Card / CardHeader

Hairline-bordered surface. `accent` prop adds an azure top line for
"primary content on this screen" emphasis. `CardHeader` auto-wraps the
title in `[ ... ]` brackets via `tf-tag` styling unless `bracketed={false}`.

### Badge

Outlined, mono, ALL-CAPS, wide tracking. Tones: `neutral · success ·
warning · danger · info · mint · violet`. Optional `dot` (filled circle)
and `pulse` (animated). Used for solver status, lambda stages, "LIVE".

### StatTile

Two layouts:
- `stack` (default) — small mono prefix above larger mono value
- `inline` — taifoon.io's `real-time 41 chains` rhythm on a single baseline

Tones colorize the number; an optional `unit` shows e.g. "ms" subtly.

### CodeBlock

Terminal block with a `$` prompt indicator, optional language tag, and a
`[ COPY ]` / `[ COPIED ]` toggle in mono. No traffic lights — kept strict
to match taifoon.io's prompt aesthetic.

### Stepper / StepBody

Vertical phase timeline. Square nodes connected by hairlines. Active node
gets a soft outline; complete nodes are solid azure. Step labels render
as `PHASE 0X — STEP` to match taifoon.io's narrative pattern.

### Tag / PhaseLabel

The two new primitives that lock the brand:

```tsx
<Tag>The engine</Tag>            →   [ THE ENGINE ]
<Tag tone="mint">Solana</Tag>    →   [ SOLANA ] (in mint)
<PhaseLabel phase={1} step="Detect" />   →   PHASE 01 — DETECT
```

### NavBar

Sticky, backdrop-blur. Triangle/peak gradient mark + `TAIFOON / SPINNERS`
wordmark (the "/ SPINNERS" disambiguates from taifoon.io). Mono ALL-CAPS
link labels. CTA is bracketed: `> SPIN_UP ▼`.

### Footer

Quiet four-column layout: brand blurb · product · protocols · hackathon.
Mono micro-caps headers, hairline dividers, terminal-flavored bottom
strip with version + license.

---

## Patterns

### Tag-and-display headline

Every section opens with a tag and a sentence-case display headline:

```tsx
<Tag>The engine</Tag>
<h2 className="tf-display tf-gradient-silver">
  One grid.
  <br />
  Every spinner.
</h2>
```

### Vertical phase timeline

The home page narrative arc uses the same vertical-line + square-node
pattern as taifoon.io's hero page, with `PHASE 0X — VERB` labels and
mint coloring on the final phase to signal Solana flavor.

### Stat callouts

Inline rhythm — small mono prefix, larger mono value:

```
real-time   41 chains
median      127ms
flywheel    ∞
```

No card chrome. Direct competition to the parent brand's pattern.

### Live indicator

A 1.5px green dot wrapped in `pulse-live` keyframes, paired with `<Badge tone="mint" dot pulse>LIVE</Badge>`. Used wherever a spinner, stream, or
pod is actively connected. Mint instead of generic green so it reads as
solver-flavored.

### Protocol pill

Outlined chip with a colored dot. Color comes from
`protocolColors[protocol-key]` in `lib/tokens.ts`. Used on the landing
marquee, solver cards, intent rows, and the onboarding picker.

---

## Voice & tone

Aligned with taifoon.io's quiet-confident-technical-poetic register:

- **Sentence case**, not ALL-CAPS, in display copy. ALL-CAPS is reserved
  for buttons, nav links, tags, and phase markers.
- **Period-separated taglines** — "One spinner. Every protocol. Every chain."
- **Bracketed labels** — `[ THE ENGINE ]`, `[ THE ECONOMY ]`, `[ THE COCKPIT ]`
- **Phases over steps** — "PHASE 04 — COMPOUND"
- **"Spinner" not "solver"** at the brand level (matches taifoon.io's
  internal lingo); "solver" is fine in technical copy.
- **Show the numbers** — 31 protocols, 38+ chains, 127ms median latency.
  Specific beats generic.

Avoid:
- Hyperbole ("the fastest", "the best"). taifoon.io doesn't reach for it.
- Emoji and shouty CSS effects.
- Heavy gradient backgrounds that compete with the type system.

---

## Routes

| Route                | Purpose                                              |
|----------------------|------------------------------------------------------|
| `/`                  | Landing — `[ THE ENGINE ]` narrative arc with phases |
| `/portal`            | Multi-spinner fleet view + onboarding launcher       |
| `/portal/[solverId]` | Live monitor — bracketed panels, mono tickers        |
| `/onboard`           | 4-phase wizard with `PHASE 0X` stepper               |
| `/docs`              | Stub linking to repo                                 |
| `/api/solver/*`      | Pre-existing SSE proxy (unchanged)                   |

---

## Open questions / next iterations

- **Logos** — current marquee uses text pills; eventually swap in real
  protocol SVG logos.
- **Empty states** — `/portal` always shows the demo fleet. Production
  needs an empty-fleet zero-state pointing at `/onboard`.
- **Mobile portal** — the live monitor is dense; consider a mobile-only
  collapsed layout.
- **Token sync** — `lib/tokens.ts` and `globals.css` are duplicated.
  A future pass could generate one from the other.
- **Logo on `/onboard` final step** — could swap the green Solana mint
  Button for a `tf-gradient-solana` border-gradient if we want a
  stronger payoff moment.
