import {defineConfig} from '@playwright/test';

const port = Number(process.env.BIBI_UI_TEST_PORT || 44901);
export default defineConfig({
  testDir: './tests/ui',
  testMatch: '**/*.spec.ts',
  fullyParallel: true,
  forbidOnly: true,
  retries: 0,
  // Avoid simultaneous graphics workloads on the interactive macOS host.
  workers: process.platform === 'darwin' ? 1 : 2,
  timeout: 60_000,
  reporter: [['list'], ['html', {open: 'never'}]],
  use: {
    baseURL: `http://127.0.0.1:${port}`,
    locale: 'ko-KR',
    reducedMotion: 'reduce',
    serviceWorkers: 'block',
    screenshot: 'only-on-failure',
    // Keep DOM/source diagnostics without continuous pixel capture after a
    // macOS WindowServer IOSurface allocation failure. Per-action ARIA snapshots
    // walk the entire transcript and exhausted long-history test budgets in CI;
    // failure error-context still includes the accessible page state.
    trace: {
      mode: 'retain-on-failure',
      screenshots: false,
      snapshots: {dom: true, aria: false, screen: false},
      sources: true,
    },
  },
  projects: [
    {name: 'chromium', use: {browserName: 'chromium'}},
    {name: 'webkit', use: {browserName: 'webkit'}},
    // Exercise Windows font/layout assumptions even when just test runs on a Mac.
    {name: 'windows-layout', testMatch: ['**/chat-tools.spec.ts','**/layout.spec.ts','**/history.spec.ts','**/theme.spec.ts','**/import.spec.ts','**/permissions.spec.ts','**/surfaces.spec.ts','**/cleanup.spec.ts','**/usage.spec.ts','**/conversation-performance.spec.ts','**/mermaid.spec.ts','**/extensions.spec.ts','**/commands.spec.ts','**/search.spec.ts','**/groups.spec.ts','**/resume.spec.ts','**/external-resume.spec.ts'], grepInvert: /titlebar/, metadata: {platform: 'Win32'}, use: {browserName: 'chromium'}},
  ],
  webServer: {
    command: 'node tests/ui/server.mjs',
    url: `http://127.0.0.1:${port}/__ready`,
    reuseExistingServer: false,
    timeout: 30_000,
    gracefulShutdown: process.platform === 'win32' ? undefined : {signal: 'SIGTERM', timeout: 5000},
  },
});
