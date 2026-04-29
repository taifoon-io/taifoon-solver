import type { MetadataRoute } from 'next'

const SITE_URL = 'https://solver.taifoon.dev'

/**
 * robots.txt — public marketing pages indexable, per-solver portal
 * routes blocked (they're operator-specific and shouldn't appear in
 * search results).
 */
export default function robots(): MetadataRoute.Robots {
  return {
    rules: [
      {
        userAgent: '*',
        allow: '/',
        disallow: [
          '/api/',
          '/portal/*/', // dynamic per-solver routes
        ],
      },
    ],
    sitemap: `${SITE_URL}/sitemap.xml`,
    host: SITE_URL,
  }
}
