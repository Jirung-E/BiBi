import {defineConfig} from '@playwright/test';

const port = Number(process.env.BIBI_UI_TEST_PORT || 44901);
export default defineConfig({
  testDir: './tests/ui',
  testMatch: '**/*.spec.ts',
  fullyParallel: true,
  forbidOnly: true,
  retries: 0,
  workers: 2,
  timeout: 60_000,
  reporter: [['list'], ['html', {open: 'never'}]],
  use: {
    baseURL: `http://127.0.0.1:${port}`,
    locale: 'ko-KR',
    reducedMotion: 'reduce',
    serviceWorkers: 'block',
    screenshot: 'only-on-failure',
    trace: 'retain-on-failure',
  },
  projects: [
    {name: 'chromium', use: {browserName: 'chromium'}},
    {name: 'webkit', use: {browserName: 'webkit'}},
  ],
  webServer: {
    command: 'node tests/ui/server.mjs',
    url: `http://127.0.0.1:${port}/__ready`,
    reuseExistingServer: false,
    timeout: 30_000,
    gracefulShutdown: process.platform === 'win32' ? undefined : {signal: 'SIGTERM', timeout: 5000},
  },
});
