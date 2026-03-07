/** @type {import('next').NextConfig} */
const nextConfig = {
  reactStrictMode: false,
  output: 'export',
  images: { unoptimized: true },
  eslint: { ignoreDuringBuilds: true },
  typescript: { ignoreBuildErrors: false },

  webpack: (config, { isServer }) => {
    if (!isServer) {
      config.resolve.fallback = {
        ...config.resolve.fallback,
        fs: false,
        path: false,
        os: false,
      };
    }

    // On the server (SSR / dev-server render pass):
    // Alias Tauri and other browser-only packages to empty modules so they
    // don't crash when webpack tries to bundle them for the Node.js runtime.
    if (isServer) {
      config.resolve.alias = {
        ...config.resolve.alias,
        // false = empty module (returns {}) — safe no-op for server builds
        '@tauri-apps/api': false,
        '@tauri-apps/api/event': false,
        '@tauri-apps/api/core': false,
        '@tauri-apps/plugin-store': false,
        '@tauri-apps/plugin-notification': false,
        '@tauri-apps/plugin-dialog': false,
        '@tauri-apps/plugin-fs': false,
        '@tauri-apps/plugin-shell': false,
        '@tauri-apps/plugin-clipboard-manager': false,
      };
    }

    return config;
  },
};

module.exports = nextConfig;
