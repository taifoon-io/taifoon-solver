import type { NextConfig } from "next";

const nextConfig: NextConfig = {
  output: 'standalone',
  basePath: '/solver',
  assetPrefix: '/solver',
};

export default nextConfig;
