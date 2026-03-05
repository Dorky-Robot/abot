import { defineConfig } from '@playwright/test';
import * as path from 'path';

const port = 7071;

export default defineConfig({
  testDir: './e2e',
  timeout: 30_000,
  expect: { timeout: 10_000 },
  globalSetup: path.resolve(__dirname, 'e2e/global-setup.ts'),
  globalTeardown: path.resolve(__dirname, 'e2e/global-teardown.ts'),
  use: {
    baseURL: `http://localhost:${port}`,
    headless: true,
    viewport: { width: 1280, height: 800 },
  },
  projects: [
    { name: 'chromium', use: { browserName: 'chromium' } },
  ],
  webServer: {
    command: `cargo run -- --data-dir \${ABOT_E2E_DATA_DIR:-/tmp/abot-e2e} --port ${port} start`,
    port,
    timeout: 120_000,
    reuseExistingServer: !process.env.CI,
  },
});
