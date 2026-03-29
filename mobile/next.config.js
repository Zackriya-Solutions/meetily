/** @type {import('next').NextConfig} */
const isDev = process.env.NODE_ENV === 'development'
const nextConfig = {
  reactStrictMode: false,
  // Static export required for Capacitor, but breaks dynamic routes in dev
  ...(isDev ? {} : { output: 'export' }),
  images: {
    unoptimized: true,
  },
  basePath: '',
  assetPrefix: '/',
  webpack: (config, { isServer }) => {
    if (!isServer) {
      config.resolve.fallback = {
        ...config.resolve.fallback,
        fs: false,
        path: false,
        os: false,
      }
    }
    // Stub out optional native Capacitor plugins not available in browser
    const path = require('path')
    const stubPath = path.resolve(__dirname, 'src/stubs/empty.js')
    config.resolve.alias = {
      ...config.resolve.alias,
      '@capacitor-community/secure-storage': stubPath,
      '@aparajita/capacitor-biometric-auth': stubPath,
    }
    return config
  },
}

module.exports = nextConfig
