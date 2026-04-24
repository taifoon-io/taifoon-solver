import type { NextConfig } from "next";

const nextConfig: NextConfig = {
  output: 'standalone',
  // basePath and assetPrefix removed - docker serves from root
};

export default nextConfig;
