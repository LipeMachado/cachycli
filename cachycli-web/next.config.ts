import type { NextConfig } from "next";

const nextConfig: NextConfig = {
  images: {
    // `ShellFrame` asks for quality 90; Next 16 only serves qualities that are
    // listed here, and avif first is what a screenshot deserves.
    qualities: [75, 90],
    formats: ["image/avif", "image/webp"],
  },
  async redirects() {
    return [
      { source: "/guide/backends", destination: "/guide/providers", permanent: true },
    ];
  },
  experimental: {
    agentFeedback: true,
  },
  cacheComponents: true,
  partialPrefetching: true,
  // Dev resources (including the HMR websocket) are same-origin by default.
  // Allow the loopback and common LAN hostnames so `npm run dev` hydrates when
  // the page is opened through 127.0.0.1 or an IP instead of localhost.
  allowedDevOrigins: ["127.0.0.1", "localhost", "192.168.68.106"],
  turbopack: {
    rules: {
      "*.css": {
        loaders: ["@tailwindcss/turbopack"],
        as: "*.css",
      },
    },
  },
};

export default nextConfig;
