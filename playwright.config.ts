import { defineConfig, devices } from "@playwright/test";
import process from "node:process";

/**
 * Runs the real components against the mock backend in a plain browser.
 * `webkit` approximates WKWebView (macOS shell), `chromium` approximates
 * WebView2 (Windows shell).
 *
 * `E2E_PORT` moves the dev server (and the preview, one port up) so two
 * checkouts can run this at once: `reuseExistingServer` would otherwise have
 * one worktree's tests attach to another worktree's server.
 */
const port = Number(process.env.E2E_PORT) || 1420;
const previewPort = port + 1;
const viewport = { width: 1800, height: 1130 };

export default defineConfig({
  testDir: "./e2e",
  fullyParallel: true,
  // WebKit can close its browser process before a test has a context, both
  // locally and on shared runners. Two retries distinguish that transient
  // runner failure from an application failure; a test that fails all three
  // times still reports a trace through `trace: "on-first-retry"` below.
  retries: 2,
  reporter: process.env.CI ? "list" : [["list"]],
  use: {
    baseURL: `http://localhost:${port}`,
    // Editing-focused scenarios represent an existing user. Fresh installs
    // still exercise the product default through unit tests and explicitly
    // enable protection in the dedicated E2E scenario.
    storageState: {
      cookies: [],
      origins: [{
        origin: `http://localhost:${port}`,
        localStorage: [{
          name: "rbl.preferences",
          value: JSON.stringify({ advanced: { protectLibrary: false } }),
        }],
      }],
    },
    trace: "on-first-retry",
    deviceScaleFactor: 2,
  },
  projects: [
    // The window matches the reference captures so screenshots compare like
    // for like. It is set per project, after the device preset: each preset
    // carries its own 1280x720 viewport, and spread later it would win.
    { name: "chromium", use: { ...devices["Desktop Chrome"], viewport } },
    { name: "webkit", use: { ...devices["Desktop Safari"], viewport } },
  ],
  webServer: [
    {
      command: `pnpm exec vite --port ${port} --strictPort`,
      url: `http://localhost:${port}`,
      reuseExistingServer: true,
      timeout: 60_000,
    },
    {
      // The built assets, which is what the shell actually ships. The dev
      // server injects inline scripts of its own, so a policy tested against
      // it would be testing Vite rather than the app.
      command: `pnpm exec vite preview --port ${previewPort} --strictPort --outDir dist`,
      url: `http://localhost:${previewPort}`,
      reuseExistingServer: true,
      timeout: 60_000,
    },
  ],
});
