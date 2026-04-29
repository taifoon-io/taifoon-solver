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

### Motion language

Lifted directly from taifoon.io's compiled CSS — same keyframes, same
durations, same opacities, same color discipline. The principle: **slow
infinite loops over flashy short ones**. Most animations cycle over 60s
to 180s so the page feels alive without ever being frantic.

| Class                      | Source            | Duration | Loop     | Effect                                                                |
|----------------------------|-------------------|---------:|----------|-----------------------------------------------------------------------|
| `.tf-grid`                 | taifoon.io hero   |    180s  | infinite | 80px square grid at 2.4% white, 3D-tilted via `matrix3d`, slides one cell  |
| `.tf-coordinate-grid`      | taifoon.io sections |   60s | infinite | 60px square grid at 3% white, slides one cell on both axes            |
| `tf-grid-curve-breathe`    | taifoon.io        |     30s  | infinite | Curved-grid scale + opacity oscillation                               |
| `.tf-orbital-trace`        | taifoon.io        |     90s  | infinite | Slow CCW rotation                                                     |
| `.tf-orbital-trace-2`      | taifoon.io        |     70s  | infinite | Slow CW rotation                                                      |
| `tf-singularity-breathe` (`.tf-breathe`) | taifoon.io |  6s | infinite | 0.55→1 opacity oscillation on central nodes                          |
| `tf-silver-shimmer`        | taifoon.io        |      8s  | infinite | Auto-applied to `.tf-gradient-silver` and `.tf-gradient-solana`       |

**Removed deliberately:** the rotating gradient stroke on buttons
(`tf-rim` / `tf-rim-rotate`). Between the hero grid drifting, two
orbital traces rotating, the volume stream sliding, and the silver
shimmer on every headline, adding rotation to CTAs pushed past
"cinematic" into "busy". Buttons hold still so the eye lands on them.

Standard motion tokens still apply for state changes:
- `--dur-fast` 120ms — hover
- `--dur-base` 200ms — state change
- `--dur-slow` 400ms — page-level transition
- `--ease-out: cubic-bezier(0.16, 1, 0.3, 1)`

**Color discipline for ambient motion** — total palette in motion layers:
- `rgba(230, 240, 247, 0.024)` — `tf-grid` line
- `rgba(230, 240, 247, 0.03)` — `tf-coordinate-grid` line
- `rgba(61, 165, 255, 0.10)` — outer orbital ring
- `rgba(20, 241, 149, 0.12)` — inner (Solana) orbital ring
- `#3DA5FF` `#14F195` — square nodes
- That's it. No protocol color leaks into ambient geometry — the multicolor riot is reserved for the LiveTicker / CrossChainVolume / IntentRow surfaces where it carries meaning.

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

### Snippet (canonical) / CodeBlock (legacy)

`Snippet` is the unified code-block primitive. Three modes, all sharing
the same prompt rhythm and copy affordance so they read as one family:

| Variant   | Use                                                      |
|-----------|----------------------------------------------------------|
| `compact` | One-line install command, no header — sits inline        |
| `default` | Fenced block with mono header (lang + `[ COPY ]`)        |
| `tabbed`  | Multi-step sequence — tabs `01 · INSTALL`, `02 · ONBOARD`, `03 · RUN` |

Visual contract: 1px hairline border, hairline divider under header,
azure `$` prompt prefix on each command line, JetBrains Mono 12px body.
Width is controlled by the parent — `Snippet` always fills its container.
This is what gives the landing, docs, and onboarding the same code-block
rhythm everywhere.

`CodeBlock` is kept around for backwards compatibility but new usage
should always be `Snippet`.

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

### NewsBand

A 36px-tall dismissable banner above the nav. Used for time-bound
announcements (hackathons, mainnet flips, talks, grants).

Anatomy, left to right:
- `[ NEWS ]` micro-cap prefix
- Tone-coded pill (`HACKATHON · MAINNET · TALK · GRANT`)
- Single-line headline
- Optional days-left ticker (computed from `endsAt`)
- `READ ↗` CTA if `href` is set
- `×` dismiss → persisted in `sessionStorage` by `id`

Adding a new entry is a one-line edit to the `NEWS` array in
`components/ui/NewsBand.tsx`. Bumping the `id` re-shows the band even
to users who dismissed the previous entry. The Colosseum hackathon is
the seed entry; when it ends it'll quietly disappear via `endsAt`.

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

### Cross-chain volume firehose (`<CrossChainVolume />`)

The hero's right column. Three sub-panels in a single bordered card:

1. **Rolling 24h counter** — total USD volume across 12 tracked
   protocols. Smooth ease-toward-target so it always feels alive but
   never frantic.
2. **60-second stacked area** — per-protocol flow over the last minute.
   Each protocol gets a colored gradient layer; the right edge has a
   mint marker showing "now". Window slides every second.
3. **Top-protocols leaderboard** — eight protocols sorted by 24h running
   total, with mini-bars sized by share-of-leader. Re-orders smoothly
   as protocols overtake each other.

**Honesty note:** the seed values in `BASELINES` are approximate
historical 24h volumes (per-protocol, USD). They are simulated forward —
each second, a per-protocol flow is generated from `daily / 86400` plus
gaussian-ish noise, with occasional 5–15× bursts to feel like real fat
orders.

The footer reads `seeded · approximate · rolling 24h · source: defillama bridges + perturbation` so users see exactly what they're looking at.

**Future work — wire to live data:**

- Daily fetch (server-side, cached) from DefiLlama Bridges API
  (`bridges.llama.fi`). Map their bridge IDs to our protocol keys, pass
  the resulting `Record<key, daily_usd>` into the `volumes` prop.
- Optional per-fill stream via the genome SSE endpoint to drive the
  60-second chart at true tx-level resolution (instead of simulating
  flow distribution from baselines).
- Replace the footer attribution with `live · defillama bridges` once
  the daily fetch is wired.

The component already accepts `volumes` as a prop, so swapping seed → live
is a one-line change at the call site.

### Infinite hero geometry (v3 — re-envisioned for true 3D)

The hero is now a **single 3D scene, single camera, single light** —
six layers, all sharing one perspective:

| # | Layer            | Plane         | Motion                                  | Color                          |
|---|------------------|---------------|-----------------------------------------|--------------------------------|
| 1 | `tf-grid`        | overhead, 1.7° X-tilt | 80px cells, 180s drift          | white at 2.4%                  |
| 2 | `tf-floor`       | floor, 55° X-tilt    | 80px cells, 12s flow toward camera | azure at 4%             |
| 3 | orbital ring A   | tilted +25° X | 90s spin, square node                   | azure at 32% (#3DA5FF node)    |
| 4 | orbital ring B   | tilted −15° X, +8° Y | 70s reverse spin, square node    | mint at 42% (#14F195 node)     |
| 5 | `tf-gyro`        | center singularity | 60s rotateY, three perpendicular rings | azure + mint              |
| 6 | link line        | ring B → center | 4s pulse, gradient stroke              | azure → mint → azure           |

**Layer 2 — perspective floor** is the "infinite" win. Strong rotateX
makes the grid lie almost flat to the viewer; a `perspective: 1200px`
parent gives real vanishing-point recession; `tf-floor-flow` advances
background-position one cell every 12s so grid lines visibly stream
toward the camera. A linear-gradient mask fades the far edge to black
so the floor never has a visible horizon line — true infinite recession.

**Layer 3 & 4 — tilted orbital rings** make the geometry feel like
*orbits*, not nested 2D circles. Each ring lives inside its own
3D-tilted wrapper; the inclination is fixed, the rotation animates on
the ring's local Z-axis (which is no longer the screen Z, so the spin
reads as an orbital path through 3D space).

**Layer 5 — `tf-gyro`** replaces the previous static center square.
Three perpendicular hairline circles (rotated to X, Y, Z planes) are
parented to a wrapper that co-rotates around Y over 60s. The Z-plane
ring is mint (the Solana-flavored axis); the X and Y rings are azure.
A breathing mint core sits at the singularity. The whole thing is
nested inside `perspective: 600px` so the rings foreshorten correctly
as they spin.

**Locked ambient palette** — strict 4-value set. Anything else in motion
layers is a defect:

| Token                              | Value                                |
|------------------------------------|--------------------------------------|
| `rgba(230, 240, 247, 0.024)`       | overhead grid line                   |
| `rgba(61, 165, 255, 0.05)`         | floor grid line                      |
| `rgba(61, 165, 255, 0.32–0.55)`    | azure orbital + gyro stroke          |
| `rgba(20, 241, 149, 0.42–0.65)`    | mint orbital + gyro stroke           |
| `#3DA5FF` `#14F195`                | square nodes + breathing core (solid)|

Multicolor protocol palette (yellow, orange, etc.) lives only in the
LiveTicker, CrossChainVolume, and IntentRow surfaces — where color
carries meaning. Ambient geometry never uses it.

### Cinematic hero (the "big bang")

The landing hero is built from four layered decorations behind two
content columns:

1. `<ChainOrbits />` — three concentric SVG orbits with chain nodes
   (Solana, Base, Arbitrum, Ethereum, Optimism, Polygon, BSC, Avalanche,
   Linea, zkSync, Scroll, Gnosis). Solana glows; the SOL↔BASE link
   pulses every 3.5s.
2. `.dot-grid` — taifoon.io's signature 24px dot pattern at 30% opacity.
3. `.glow-bg` — radial gradients in azure (top-right) and mint (bottom-left).
4. Radial vignette — fades the orbits at the edges so type stays readable.

Content sits in two columns:

- **Left**: silver-gradient display headline ("One spinner / Every protocol /
  Every chain"), where "Every protocol" gets the `tf-gradient-solana`
  blue→mint treatment for emphasis. Below the CTAs sits a four-tile KPI
  strip — "$ FILLED · 24H" and "FILLS · 24H" use `<Counter />` to tick up
  in real time. This is the "proof, not pitch" payoff.

- **Right**: `<LiveTicker />` — a streaming intent feed that adds a row
  every ~1.6s, slides the oldest off the bottom, fades the bottom edge
  to suggest the stream continues. New rows briefly flash mint as they
  land. Below the ticker, a footer reads "Proof-of-runtime · not a mockup"
  with a `SEE LIVE PORTAL →` link. This is the moment we earn the
  "world-class" framing — most competing landings show a static image.

Why this works: the brand promise is autonomy + speed. A still hero
undersells that promise. By making the runtime visibly alive in the
first 2 seconds, the page stops looking like marketing and starts looking
like the product.

The same `<LiveTicker />` is reused inside the "cockpit" preview section
further down the page, ensuring the dashboard preview also moves rather
than sitting still.

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
