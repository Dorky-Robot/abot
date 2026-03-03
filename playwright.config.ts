import { defineConfig } from '@playwright/test';

const port = process.env.ABOT_PORT ?? '7070';

export default defineConfig({
  testDir: './e2e',
  timeout: 30_000,
  expect: { timeout: 10_000 },
  use: {
    baseURL: `http://localhost:${port}`,
    // Default headless; override with --headed for debugging.
    // Flutter WASM canvas may need headed mode for visual tests,
    // but API-level tests work fine headless.
    headless: true,
    viewport: { width: 1280, height: 800 },
  },
  projects: [
    { name: 'chromium', use: { browserName: 'chromium' } },
  ],
});
