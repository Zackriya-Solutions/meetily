import { defineConfig } from 'vitest/config'
import react from '@vitejs/plugin-react'
import path from 'path'

export default defineConfig({
  plugins: [react()],
  test: {
    environment: 'jsdom',
    globals: true,
    setupFiles: ['./src/__tests__/setup.ts'],
    include: ['src/**/*.test.{ts,tsx}'],
    coverage: {
      provider: 'v8',
      include: ['src/**/*.{ts,tsx}'],
      exclude: [
        'src/__tests__/**',
        'src/app/globals.css',
        'src/**/*.d.ts',
      ],
      thresholds: {
        statements: 90,
        branches: 85,
        functions: 90,
        lines: 90,
      },
    },
  },
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
      // These packages are not installed but are dynamically imported in source.
      // Alias them so Vite's import-analysis can resolve them; vi.mock() in
      // setup.ts intercepts the actual module at runtime.
      '@capacitor-community/secure-storage': path.resolve(__dirname, './src/__tests__/stubs/empty.ts'),
      '@aparajita/capacitor-biometric-auth': path.resolve(__dirname, './src/__tests__/stubs/empty.ts'),
      '@capacitor-community/sqlite': path.resolve(__dirname, './src/__tests__/stubs/empty.ts'),
      '@capacitor/local-notifications': path.resolve(__dirname, './src/__tests__/stubs/empty.ts'),
    },
  },
})
