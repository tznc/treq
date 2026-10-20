// The landing screenshot capture (`npm run landing`), kept out of the e2e
// suite's `testDir`. It drives the app on the demo repository `launch.ts`
// seeds and writes each PNG named in web/src/data/landing-screenshots.json
// into web/static/img/landing/.
import { defineConfig } from "@playwright/test";
import { port } from "../env.ts";

export default defineConfig({
  testDir: ".",
  testMatch: "capture.spec.ts",
  workers: 1,
  fullyParallel: false,
  forbidOnly: !!process.env.CI,
  timeout: 240_000,
  reporter: "list",
  expect: { timeout: 15_000 },
  outputDir: "../test-results/landing",
  use: {
    connectOptions: { wsEndpoint: `ws://127.0.0.1:${port}/` },
  },
  webServer: {
    command: "node landing/launch.ts",
    cwd: "..",
    url: `http://127.0.0.1:${port}/`,
    reuseExistingServer: false,
    timeout: 180_000,
    stdout: "pipe",
    stderr: "pipe",
  },
});
