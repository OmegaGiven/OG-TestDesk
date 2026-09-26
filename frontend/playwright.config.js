import { defineConfig, devices } from '@playwright/test';

// E2E smoke tests run the real frontend build against mockTauri.js (the
// in-browser backend stand-in), so they exercise every Svelte view,
// store and dialog without needing the Rust side. `pnpm test:e2e` builds
// first; CI builds in an earlier step and runs `playwright test` directly.
export default defineConfig({
  testDir: 'tests/e2e',
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 1 : 0,
  reporter: process.env.CI ? [['github'], ['html', { open: 'never' }]] : 'list',
  use: {
    baseURL: 'http://localhost:4173',
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure',
    viewport: { width: 1300, height: 900 }
  },
  projects: [{ name: 'chromium', use: { ...devices['Desktop Chrome'], viewport: { width: 1300, height: 900 } } }],
  webServer: {
    command: 'pnpm exec vite preview --port 4173 --strictPort',
    url: 'http://localhost:4173',
    reuseExistingServer: !process.env.CI,
    timeout: 60_000
  }
});
