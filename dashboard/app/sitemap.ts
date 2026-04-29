import type { MetadataRoute } from 'next'

const SITE_URL = 'https://solver.taifoon.dev'

/**
 * Static sitemap for the marketing surface. The /portal/[solverId]
 * routes are dynamic per-operator and intentionally excluded.
 */
export default function sitemap(): MetadataRoute.Sitemap {
  const now = new Date()
  return [
    {
      url: `${SITE_URL}/`,
      lastModified: now,
      changeFrequency: 'weekly',
      priority: 1,
    },
    {
      url: `${SITE_URL}/onboard`,
      lastModified: now,
      changeFrequency: 'monthly',
      priority: 0.9,
    },
    {
      url: `${SITE_URL}/portal`,
      lastModified: now,
      changeFrequency: 'daily',
      priority: 0.8,
    },
    {
      url: `${SITE_URL}/docs`,
      lastModified: now,
      changeFrequency: 'weekly',
      priority: 0.7,
    },
  ]
}
