// The visual QA run (`npm run qa`), kept out of the e2e suite's `testDir`.
// It drives the app on the demo repository `launch.ts` seeds and writes a
// PNG plus a JSON of what to check for every screen and state into
// `QA_OUT` (default qa/.shots/). `QA_THEME=light` runs it in the light
// theme; `npm run qa -- --grep commits` runs one area. `QA_REUSE=1` drives
// an app that is already running on the port instead of starting one.
import { defineConfig } from "@playwright/test";
import { port } from "../env.ts";

export default defineConfig({
  testDir: ".",
  testMatch: "*.spec.ts",
  workers: 1,
  fullyParallel: false,
  forbidOnly: !!process.env.CI,
  timeout: 300_000,
  reporter: "list",
  expect: { timeout: 15_000 },
  outputDir: "../test-results/qa",
  use: {
    connectOptions: { wsEndpoint: `ws://127.0.0.1:${port}/` },
    actionTimeout: 20_000,
  },
  webServer: {
    command: "node qa/launch.ts",
    cwd: "..",
    url: `http://127.0.0.1:${port}/`,
    reuseExistingServer: !!process.env.QA_REUSE,
    timeout: 180_000,
    stdout: "pipe",
    stderr: "pipe",
  },
});
