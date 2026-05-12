/**
 * /policy — Public donut-policy + adapter-registry page.
 *
 * Server-rendered. Reads the same two endpoints every auditor reads:
 *
 *   GET /api/donut/policy    → canonical 49 bps × 70 / 20 / 10 constants
 *   GET /api/donut/registry  → adapter → builder + reviewer map
 *
 * The page renders the three public-audit-layer artifacts described in
 * docs/donut_flow.md §5 — the canonical constants, the per-adapter
 * builder mapping, and the ledger pointer — so anyone can confirm the
 * "uniform across all builders" claim without trusting the Spinner.
 *
 * No client-side wallet libraries here: this is a read-only public page.
 */
import { revalidatePath } from 'next/cache'
import { NavBar, Footer, Card, CardHeader, StatTile, Tag, Badge } from '@/components/ui'

// ── Endpoint resolution ───────────────────────────────────────────────
//
// Mirrors the pattern used everywhere else in the dashboard: a
// server-side internal URL (preferred when the dashboard is co-located
// with solver-api), falling back to the public NEXT_PUBLIC_ variable
// when set, then localhost. This page is SSR — no `'use client'` — so
// either var works.
const SOLVER_API_URL =
  process.env.SOLVER_API_INTERNAL_URL ??
  process.env.NEXT_PUBLIC_SOLVER_API_URL ??
  'http://127.0.0.1:8082'

// Canonical fee-split shape published by /api/donut/policy.
interface DonutPolicy {
  donut_bps_num: number
  donut_bps_den: number
  creator_num: number
  reviewer_num: number
  ecosystem_num: number
  split_den: number
  micro_usd_per_usd: number
  applies_to: string
  adjudicator_version: string
}

// Single registry entry as published by /api/donut/registry.
interface AdapterEntry {
  builder: string
  reviewers: string[]
  donut_bps_num: number | null
  donut_bps_den: number | null
}

interface RegistryView {
  ecosystem: string
  adapters: Record<string, AdapterEntry>
}

// `0x0000…0000` (40 hex zeros) is the fail-closed default. solver-api
// installs this when ADAPTER_REGISTRY_PATH points at a missing or
// unreadable file — anything routing to that address would burn the
// Builder + reviewer shares, so we render a loud WARN banner instead of
// pretending the registry is valid.
const ZERO_ADDRESS = '0x' + '0'.repeat(40)
function isZeroAddress(addr: string): boolean {
  return addr.toLowerCase() === ZERO_ADDRESS
}

function shortAddr(addr: string): string {
  if (!addr || addr.length < 12) return addr
  return `${addr.slice(0, 6)}…${addr.slice(-4)}`
}

// 49 / 10_000 → "49 bps". Computed defensively in case the server ever
// publishes a different (numerator, denominator) pair: we always show
// the math the canonical adjudicator does, not a hard-coded literal.
function bpsLabel(num: number, den: number): string {
  if (den === 0) return '— bps'
  const bps = (num / den) * 10_000
  // Whole-number bps render without a decimal; fractional rates (rare
  // — would require an adapter-specific override) keep two places.
  return Number.isInteger(bps) ? `${bps} bps` : `${bps.toFixed(2)} bps`
}

// Fetch both endpoints in parallel. `cache: 'no-store'` keeps the page
// always-fresh — the registry is a runtime config artifact and we want
// any reload to surface a new ecosystem address or adapter immediately.
async function fetchPolicyAndRegistry(): Promise<{
  policy: DonutPolicy | null
  registry: RegistryView | null
  error: string | null
}> {
  try {
    const [policyRes, registryRes] = await Promise.all([
      fetch(`${SOLVER_API_URL}/api/donut/policy`, { cache: 'no-store' }),
      fetch(`${SOLVER_API_URL}/api/donut/registry`, { cache: 'no-store' }),
    ])
    if (!policyRes.ok) {
      return {
        policy: null,
        registry: null,
        error: `policy endpoint returned HTTP ${policyRes.status}`,
      }
    }
    if (!registryRes.ok) {
      return {
        policy: null,
        registry: null,
        error: `registry endpoint returned HTTP ${registryRes.status}`,
      }
    }
    const policy = (await policyRes.json()) as DonutPolicy
    const registry = (await registryRes.json()) as RegistryView
    return { policy, registry, error: null }
  } catch (e) {
    return {
      policy: null,
      registry: null,
      error: e instanceof Error ? e.message : String(e),
    }
  }
}

// Server Action — wired to the Retry button on the error panel. Just
// revalidates this path so the next render re-fetches both endpoints.
async function retryFetch() {
  'use server'
  revalidatePath('/policy')
}

export default async function PolicyPage() {
  const { policy, registry, error } = await fetchPolicyAndRegistry()

  return (
    <>
      <NavBar />
      <main className="flex-1">
        <div className="max-w-[1100px] mx-auto px-6 py-12">
          {/* ── Header ──────────────────────────────────────────── */}
          <div className="mb-10 flex items-start justify-between gap-6 flex-wrap">
            <div>
              <Tag>The policy</Tag>
              <h1 className="tf-display tf-gradient-silver mt-4 text-[clamp(2rem,4vw,3rem)]">
                TSUL Donut Policy.
              </h1>
              <p className="mt-3 text-[var(--text-secondary)] max-w-[620px] leading-relaxed">
                The fee split applied uniformly across every provisioned
                adapter. Reads straight from{' '}
                <code className="font-mono text-[var(--brand-blue)]">/api/donut/policy</code>{' '}
                and{' '}
                <code className="font-mono text-[var(--brand-blue)]">/api/donut/registry</code>{' '}
                — the same endpoints any auditor uses.
              </p>
            </div>
            {policy && (
              <Badge tone="info" className="mt-2">
                {policy.adjudicator_version}
              </Badge>
            )}
          </div>

          {/* ── Error panel ─────────────────────────────────────── */}
          {error && (
            <Card padding="lg" className="border-[var(--danger)]/30">
              <div className="flex items-start gap-4">
                <div className="w-10 h-10 rounded-full border border-[var(--danger)]/40 flex items-center justify-center bg-[rgba(255,107,107,0.05)] shrink-0">
                  <span className="text-[var(--danger)] font-mono text-lg">!</span>
                </div>
                <div className="flex-1 min-w-0">
                  <div className="text-sm text-[var(--danger)] font-mono">
                    Policy endpoint unreachable
                  </div>
                  <div className="text-[11px] text-[var(--text-tertiary)] mt-1 font-mono break-all">
                    {error}
                  </div>
                  <div className="text-[11px] text-[var(--text-tertiary)] mt-2">
                    Start the solver with{' '}
                    <code className="bg-[var(--bg-raised)] px-1 py-0.5 rounded font-mono">
                      cargo run --bin taifoon-solver
                    </code>{' '}
                    or set{' '}
                    <code className="bg-[var(--bg-raised)] px-1 py-0.5 rounded font-mono">
                      SOLVER_API_INTERNAL_URL
                    </code>
                    .
                  </div>
                  <form action={retryFetch} className="mt-4">
                    <button
                      type="submit"
                      className="inline-flex items-center justify-center gap-2 h-9 px-4 font-mono text-[12px] tracking-[0.01em] border border-[var(--brand-blue)]/60 text-[var(--brand-blue)] hover:bg-[var(--brand-blue)]/10 hover:border-[var(--brand-blue)] rounded-[var(--r-sm)] transition-all"
                    >
                      Retry
                    </button>
                  </form>
                </div>
              </div>
            </Card>
          )}

          {/* ── Fail-closed warning ─────────────────────────────── */}
          {registry && isZeroAddress(registry.ecosystem) && (
            <Card padding="md" className="border-[var(--warning)]/40 bg-[var(--warning)]/5 mb-8">
              <div className="flex items-start gap-3">
                <span className="text-[var(--warning)] font-mono text-base shrink-0">!</span>
                <div>
                  <div className="text-[10px] uppercase tracking-[0.2em] text-[var(--warning)] font-bold mb-1">
                    Fail-closed: registry not loaded
                  </div>
                  <p className="text-[12px] text-[var(--text-secondary)] leading-relaxed">
                    The Spinner OS hasn&apos;t loaded an{' '}
                    <code className="font-mono text-[var(--warning)]">adapter_registry.json</code>{' '}
                    — Builder shares are currently routed to the zero address.
                    Fix{' '}
                    <code className="font-mono text-[var(--warning)]">ADAPTER_REGISTRY_PATH</code>{' '}
                    before going live.
                  </p>
                </div>
              </div>
            </Card>
          )}

          {/* ── 1. Canonical policy panel ──────────────────────── */}
          {policy && (
            <Card padding="lg" accent className="mb-8">
              <CardHeader
                title="Canonical policy"
                subtitle="Pinned per attestation, re-verified by every reader. A Spinner cannot quietly change the policy on their own fills."
              />
              <div className="grid grid-cols-2 sm:grid-cols-4 gap-x-8 gap-y-6 mt-2">
                <StatTile
                  label="Donut rate"
                  value={bpsLabel(policy.donut_bps_num, policy.donut_bps_den)}
                  tone="blue"
                />
                <StatTile
                  label="Creator share"
                  value={`${Math.round((policy.creator_num / policy.split_den) * 100)}%`}
                  tone="mint"
                />
                <StatTile
                  label="Reviewer share"
                  value={`${Math.round((policy.reviewer_num / policy.split_den) * 100)}%`}
                  tone="violet"
                />
                <StatTile
                  label="Ecosystem share"
                  value={`${Math.round((policy.ecosystem_num / policy.split_den) * 100)}%`}
                />
              </div>

              {/* Raw constants strip — keeps the numerator/denominator
                  visible so auditors don't have to trust the % rounding. */}
              <div className="mt-6 grid grid-cols-2 sm:grid-cols-4 gap-3 text-[11px] font-mono text-[var(--text-tertiary)]">
                <ConstantPair label="bps_num" value={policy.donut_bps_num} />
                <ConstantPair label="bps_den" value={policy.donut_bps_den} />
                <ConstantPair label="split_den" value={policy.split_den} />
                <ConstantPair
                  label="micro_usd_per_usd"
                  value={policy.micro_usd_per_usd.toLocaleString()}
                />
              </div>

              <p className="mt-6 text-[12px] text-[var(--text-secondary)] leading-relaxed max-w-[760px]">
                The{' '}
                <span className="text-[var(--brand-blue)] font-mono">
                  {bpsLabel(policy.donut_bps_num, policy.donut_bps_den)}
                </span>{' '}
                rate is a default. Individual adapters may declare a
                different rate in their registry entry (visible in the table
                below). The{' '}
                <span className="text-[var(--solana-mint)] font-mono">
                  {Math.round((policy.creator_num / policy.split_den) * 100)} /{' '}
                  {Math.round((policy.reviewer_num / policy.split_den) * 100)} /{' '}
                  {Math.round((policy.ecosystem_num / policy.split_den) * 100)}
                </span>{' '}
                split is uniform and not overridable. Applies to{' '}
                <code className="font-mono text-[var(--text-primary)]">
                  {policy.applies_to}
                </code>
                .
              </p>
            </Card>
          )}

          {/* ── 2. Adapter registry table ───────────────────────── */}
          {registry && policy && (
            <Card padding="lg" className="mb-8">
              <CardHeader
                title="Adapter registry"
                subtitle={`${Object.keys(registry.adapters).length} adapter${
                  Object.keys(registry.adapters).length === 1 ? '' : 's'
                } provisioned. Sorted alphabetically by adapter_id.`}
              />
              <AdapterTable
                adapters={registry.adapters}
                defaultBpsNum={policy.donut_bps_num}
                defaultBpsDen={policy.donut_bps_den}
              />
            </Card>
          )}

          {/* ── 3. Ecosystem footer ─────────────────────────────── */}
          {registry && (
            <Card padding="lg" className="mb-8">
              <CardHeader
                title="Ecosystem treasury"
                subtitle="Receives 10% of every donut. Also catches the 70% + 20% on fills routed through unregistered adapters (fail-closed)."
              />
              <div className="flex items-center gap-3 flex-wrap">
                <span
                  className="font-mono text-[13px] text-[var(--text-primary)] break-all select-all bg-[var(--bg-raised)] border border-[var(--border-default)] rounded-[var(--r-sm)] px-3 py-2"
                  title={registry.ecosystem}
                >
                  {registry.ecosystem}
                </span>
                {isZeroAddress(registry.ecosystem) && (
                  <Badge tone="warning">ZERO — fail-closed</Badge>
                )}
              </div>
            </Card>
          )}

          {/* ── 4. Audit-trail link ─────────────────────────────── */}
          <Card padding="md" className="mb-12">
            <CardHeader title="Audit trail" bracketed />
            <p className="text-[12px] text-[var(--text-secondary)] leading-relaxed mb-3">
              Verify any Spinner&apos;s signed attestation chain — the
              public-audit layer described in{' '}
              <code className="font-mono text-[var(--brand-blue)]">
                docs/donut_flow.md §5
              </code>
              .
            </p>
            <pre className="rounded-[var(--r-sm)] border border-[var(--border-default)] bg-[var(--bg-raised)] px-4 py-3 text-[12px] font-mono text-[var(--text-primary)] overflow-x-auto">
              <code>
                <span className="text-[var(--brand-blue)]">GET</span>{' '}
                /api/donut/ledger/
                <span className="text-[var(--solana-mint)]">
                  &lt;your_spinner_id&gt;
                </span>
              </code>
            </pre>
          </Card>
        </div>
      </main>
      <Footer />
    </>
  )
}

// ── Helpers ────────────────────────────────────────────────────────────

function ConstantPair({ label, value }: { label: string; value: number | string }) {
  return (
    <div className="flex items-baseline gap-2">
      <span className="text-[var(--text-tertiary)]">{label}</span>
      <span className="text-[var(--text-secondary)] tabular-nums">{value}</span>
    </div>
  )
}

/**
 * AdapterTable — sorted, striped table of `adapter_id → builder/reviewer/bps`.
 *
 * We render the override column in mint when an adapter declares its own
 * rate so an auditor scanning the column can spot anything that diverges
 * from the canonical 49 bps default at a glance. Full addresses are
 * exposed via `title=` for copy-on-hover; the visible label is the
 * standard `0x1111…aaaa` truncation.
 */
function AdapterTable({
  adapters,
  defaultBpsNum,
  defaultBpsDen,
}: {
  adapters: Record<string, AdapterEntry>
  defaultBpsNum: number
  defaultBpsDen: number
}) {
  const sorted = Object.entries(adapters).sort(([a], [b]) => a.localeCompare(b))

  if (sorted.length === 0) {
    return (
      <div className="text-center py-12 text-[var(--text-secondary)] text-[13px]">
        No adapters provisioned.
        <div className="mt-2 text-[11px] text-[var(--text-tertiary)] font-mono">
          Check{' '}
          <code className="bg-[var(--bg-raised)] px-1 py-0.5 rounded">
            config/adapter_registry.json
          </code>
        </div>
      </div>
    )
  }

  return (
    <div className="overflow-x-auto -mx-2">
      <table className="w-full text-[12px] font-mono">
        <thead>
          <tr className="border-b border-[var(--border-default)]">
            <Th>adapter_id</Th>
            <Th>builder</Th>
            <Th>reviewers</Th>
            <Th>donut rate</Th>
            <Th>solana</Th>
          </tr>
        </thead>
        <tbody>
          {sorted.map(([id, entry], i) => {
            const hasOverride =
              entry.donut_bps_num !== null && entry.donut_bps_den !== null
            const effectiveNum = entry.donut_bps_num ?? defaultBpsNum
            const effectiveDen = entry.donut_bps_den ?? defaultBpsDen
            const isSolana = id.toLowerCase().includes('solana')
            const builderZero = isZeroAddress(entry.builder)
            return (
              <tr
                key={id}
                className={`border-b border-[var(--border-subtle)] ${
                  i % 2 === 1 ? 'bg-[var(--bg-raised)]/30' : ''
                }`}
              >
                <Td>
                  <span className="text-[var(--text-primary)]">{id}</span>
                </Td>
                <Td>
                  <span
                    className={
                      builderZero
                        ? 'text-[var(--warning)]'
                        : 'text-[var(--text-secondary)]'
                    }
                    title={entry.builder}
                  >
                    {shortAddr(entry.builder)}
                  </span>
                </Td>
                <Td>
                  <span className="text-[var(--text-secondary)]">
                    {entry.reviewers.length} reviewer
                    {entry.reviewers.length === 1 ? '' : 's'}
                  </span>
                </Td>
                <Td>
                  <span
                    className={
                      hasOverride
                        ? 'text-[var(--solana-mint)]'
                        : 'text-[var(--text-secondary)]'
                    }
                    title={
                      hasOverride
                        ? `override ${entry.donut_bps_num}/${entry.donut_bps_den}`
                        : `default ${defaultBpsNum}/${defaultBpsDen}`
                    }
                  >
                    {bpsLabel(effectiveNum, effectiveDen)}
                    {hasOverride && (
                      <span className="ml-1.5 text-[9px] uppercase tracking-[0.18em]">
                        override
                      </span>
                    )}
                  </span>
                </Td>
                <Td>
                  {isSolana ? (
                    <Badge tone="violet">SVM</Badge>
                  ) : (
                    <span className="text-[var(--text-tertiary)]">—</span>
                  )}
                </Td>
              </tr>
            )
          })}
        </tbody>
      </table>
    </div>
  )
}

function Th({ children }: { children: React.ReactNode }) {
  return (
    <th className="text-left px-2 py-2.5 text-[10px] uppercase tracking-[0.2em] text-[var(--text-tertiary)] font-normal">
      {children}
    </th>
  )
}

function Td({ children }: { children: React.ReactNode }) {
  return <td className="px-2 py-3 align-middle">{children}</td>
}
