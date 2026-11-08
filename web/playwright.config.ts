import { defineConfig } from '@playwright/test';
export default defineConfig({
  testDir: './e2e', workers: 1, timeout: 60_000,
  use: { baseURL: process.env.CACHECAT_URL ?? 'http://127.0.0.1:5188', channel: process.env.CI ? 'chromium' : 'msedge', viewport: { width: 1280, height: 900 }, screenshot: 'only-on-failure' },
});
