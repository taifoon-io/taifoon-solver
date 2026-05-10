import { NextResponse } from 'next/server'

const SOLVER_API_URL =
  process.env.SOLVER_API_INTERNAL_URL ?? 'http://127.0.0.1:8082'

// GET /api/solvers — fleet-level summary of all solvers known to this instance.
// For now the solver-api manages exactly one solver, so we proxy the known
// endpoints and assemble a fleet response. This is the real endpoint that
// /portal's "Add new solver" flow will POST to later.
export async function GET() {
  try {
    const [portfolioRes, pnlRes, statsRes] = await Promise.allSettled([
      fetch(`${SOLVER_API_URL}/api/solver/portfolio`, { cache: 'no-store' }),
      fetch(`${SOLVER_API_URL}/api/solver/pnl`, { cache: 'no-store' }),
      fetch(`${SOLVER_API_URL}/api/solver/stats`, { cache: 'no-store' }),
    ])

    const portfolio =
      portfolioRes.status === 'fulfilled' && portfolioRes.value.ok
        ? await portfolioRes.value.json()
        : null

    const pnl =
      pnlRes.status === 'fulfilled' && pnlRes.value.ok
        ? await pnlRes.value.json()
        : null

    const stats =
      statsRes.status === 'fulfilled' && statsRes.value.ok
        ? await statsRes.value.json()
        : null

    if (!portfolio) {
      return NextResponse.json({ solvers: [], count: 0 })
    }

    const solver = {
      id: portfolio.solver_address?.slice(2, 10)?.toLowerCase() ?? 'unknown',
      address: portfolio.solver_address ?? null,
      solana_address: portfolio.solana_address ?? null,
      chains: portfolio.chains ?? [],
      fills: portfolio.fills ?? null,
      pnl: pnl
        ? {
            realized_usd_total: pnl.realized_usd_total ?? 0,
            fills_total: pnl.fills_total ?? 0,
            last_24h_count: pnl.last_24h_count ?? 0,
            by_protocol: pnl.by_protocol ?? {},
          }
        : null,
      stats: stats ?? null,
      as_of: portfolio.as_of ?? new Date().toISOString(),
    }

    return NextResponse.json(
      { solvers: [solver], count: 1 },
      { headers: { 'Cache-Control': 'no-store' } },
    )
  } catch (e) {
    return NextResponse.json(
      { error: e instanceof Error ? e.message : 'internal error', solvers: [], count: 0 },
      { status: 500 },
    )
  }
}
