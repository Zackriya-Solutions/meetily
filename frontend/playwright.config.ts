import { defineConfig } from '@playwright/test';

export default defineConfig({
  testDir: './tests/e2e',
  outputDir: process.env.PLAYWRIGHT_OUTPUT_DIR || '/tmp/meetily-theme-test-results',
  workers: 1,
  reporter: [['list'], ['json', { outputFile: '/tmp/meetily-theme-test-results/results.json' }]],
  use: {
    baseURL: 'http://127.0.0.1:3118',
    viewport: { width: 1280, height: 900 },
    video: 'on',
    trace: 'retain-on-failure',
  },
  webServer: {
    command: 'node node_modules/next/dist/bin/next dev -H 127.0.0.1 -p 3118',
    url: 'http://127.0.0.1:3118',
    reuseExistingServer: !process.env.CI,
    timeout: 120000,
  },
});
