/** @type {import('next').NextConfig} */
const nextConfig = {
  // Static export: the daemon serves this directory from disk. No server runtime, no cloud.
  output: "export",
  images: { unoptimized: true },
  trailingSlash: true,
};

export default nextConfig;
