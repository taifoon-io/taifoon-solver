import { NextResponse } from 'next/server'

// Cache the DefiLlama response for 5 minutes server-side
// so every visitor doesn't trigger a new upstream call.
const CACHE_TTL_MS = 5 * 60 * 1000

let cachedData: Record<string, number> | null = null
let cacheAt = 0

// Maps DefiLlama bridge names to our internal protocol keys
const BRIDGE_NAME_MAP: Record<string, string> = {
  'Wormhole':        'wormhole',
  'CCTP':            'cctp',
  'Across Protocol': 'across',
  'Across':          'across',
  'Li.Fi':           'lifi',
  'Stargate':        'stargate',
  'deBridge':        'debridge',
  'Mayan':           'mayan',
  'Synapse':         'synapse',
  'Orbiter Finance': 'orbiter',
  'Hop Protocol':    'hop',
  'Squid':           'squid',
  'Symbiosis':       'symbiosis',
}

async function fetchDefiLlama(): Promise<Record<string, number>> {
  const res = await fetch('https://bridges.llama.fi/bridges?includeChains=true', {
    next: { revalidate: 300 },
    headers: { 'User-Agent': 'taifoon-solver-dashboard/1.0' },
  })
  if (!res.ok) throw new Error(`DefiLlama HTTP ${res.status}`)
  const json = await res.json()

  const volumes: Record<string, number> = {}
  for (const bridge of json.bridges ?? []) {
    const key = BRIDGE_NAME_MAP[bridge.displayName] ?? BRIDGE_NAME_MAP[bridge.name]
    if (key && bridge.lastDailyVolume != null) {
      volumes[key] = (volumes[key] ?? 0) + Number(bridge.lastDailyVolume)
    }
  }
  return volumes
}

export async function GET() {
  const now = Date.now()
  if (cachedData && now - cacheAt < CACHE_TTL_MS) {
    return NextResponse.json(cachedData, {
      headers: {
        'Cache-Control': 'public, s-maxage=300, stale-while-revalidate=60',
        'X-Data-Source': 'defillama-cached',
      },
    })
  }

  try {
    const volumes = await fetchDefiLlama()
    cachedData = volumes
    cacheAt = now
    return NextResponse.json(volumes, {
      headers: {
        'Cache-Control': 'public, s-maxage=300, stale-while-revalidate=60',
        'X-Data-Source': 'defillama-live',
      },
    })
  } catch (e) {
    // On error, return stale cache if available, otherwise empty
    if (cachedData) {
      return NextResponse.json(cachedData, {
        headers: {
          'Cache-Control': 'public, s-maxage=60',
          'X-Data-Source': 'defillama-stale',
        },
      })
    }
    return NextResponse.json(
      { error: e instanceof Error ? e.message : 'fetch failed' },
      { status: 502 },
    )
  }
}
