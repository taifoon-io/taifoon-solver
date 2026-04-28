import type { NextConfig } from "next";

const solverApi = process.env.SOLVER_API_INTERNAL_URL ?? 'http://127.0.0.1:8099';

const nextConfig: NextConfig = {
  output: 'standalone',
  async rewrites() {
    return [
      {
        source: '/api/solver/:path*',
        destination: `${solverApi}/api/solver/:path*`,
      },
    ];
  },
};

export default nextConfig;
