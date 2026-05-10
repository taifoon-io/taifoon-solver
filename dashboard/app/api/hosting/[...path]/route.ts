import { type NextRequest, NextResponse } from 'next/server'

const UPSTREAM = process.env.SOLVER_API_INTERNAL_URL ?? 'http://127.0.0.1:8082'

async function proxy(req: NextRequest, { params }: { params: Promise<{ path: string[] }> }) {
  const { path } = await params
  const url = `${UPSTREAM}/api/hosting/${path.join('/')}${req.nextUrl.search}`
  try {
    const upstream = await fetch(url, {
      method: req.method,
      headers: { 'content-type': req.headers.get('content-type') ?? 'application/json' },
      body: req.method !== 'GET' ? await req.text() : undefined,
    })
    return new NextResponse(upstream.body, {
      status: upstream.status,
      headers: {
        'content-type': upstream.headers.get('content-type') ?? 'application/json',
        'cache-control': 'no-store',
        'access-control-allow-origin': '*',
      },
    })
  } catch (e: any) {
    return NextResponse.json({ error: e?.message }, { status: 502 })
  }
}

export const GET = proxy
export const POST = proxy
