import { defineConfig } from "@playwright/test";

/**
 * Browser evidence deliberately runs against built, same-origin applications.
 * Each test installs an in-memory BFF fixture before navigation; no test has a
 * database, IdP, mail engine, or production-network dependency.
 */
export default defineConfig({
  testDir: "./e2e",
  outputDir: "./test-results",
  // Browser screenshots run on Windows locally and in the dedicated Windows
  // CI job. Keep one reviewable baseline path inside that pinned OS family;
  // unit/build gates continue to run independently on Linux.
  snapshotPathTemplate: "{testDir}/{testFilePath}-snapshots/{arg}{ext}",
  fullyParallel: true,
  forbidOnly: Boolean(process.env.CI),
  retries: process.env.CI ? 1 : 0,
  // Full-page visual baselines and Axe scans are Chromium-memory intensive.
  // Keep the local gate deterministic instead of letting Playwright expand to
  // every host core; CI remains deliberately parallel but bounded.
  workers: process.env.CI ? 2 : 1,
  reporter: [
    ["list"],
    ["html", { open: "never", outputFolder: "playwright-report" }],
  ],
  use: {
    colorScheme: "light",
    contextOptions: { reducedMotion: "reduce" },
    deviceScaleFactor: 1,
    locale: "en-US",
    serviceWorkers: "block",
    screenshot: "only-on-failure",
    trace: "retain-on-failure",
    video: "retain-on-failure",
    timezoneId: "UTC",
  },
  expect: {
    timeout: 10_000,
    toHaveScreenshot: {
      animations: "disabled",
      caret: "hide",
      maxDiffPixelRatio: 0.005,
    },
  },
  projects: [
    {
      name: "admin",
      testMatch: "admin/**/*.spec.ts",
      use: { baseURL: "http://127.0.0.1:4173" },
    },
    {
      name: "mail",
      testMatch: "mail/**/*.spec.ts",
      use: { baseURL: "http://127.0.0.1:4174" },
    },
  ],
  webServer: [
    {
      command: "node node_modules/vite/bin/vite.js preview admin-app --host 127.0.0.1 --port 4173 --strictPort",
      url: "http://127.0.0.1:4173/",
      reuseExistingServer: !process.env.CI,
      timeout: 30_000,
    },
    {
      command: "node node_modules/vite/bin/vite.js preview mail-app --host 127.0.0.1 --port 4174 --strictPort",
      url: "http://127.0.0.1:4174/",
      reuseExistingServer: !process.env.CI,
      timeout: 30_000,
    },
  ],
});
