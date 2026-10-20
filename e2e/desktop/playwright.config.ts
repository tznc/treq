// Stock @playwright/test driving the desktop app through gpui-playwright:
// no browser is launched, the app is the Playwright server. `launch.ts`
// starts it on a fixture repository; see README.md for the build and the
// Xvfb command.
import { defineConfig } from "@playwright/test";
import { port } from "./env.ts";

export default defineConfig({
  testDir: "./tests",
  // One app, one window: the tests share it and run in file order.
  workers: 1,
  fullyParallel: false,
  forbidOnly: !!process.env.CI,
  timeout: 60_000,
  reporter: process.env.CI ? [["list"], ["html", { open: "never" }]] : "list",
  expect: { timeout: 10_000 },
  use: {
    connectOptions: { wsEndpoint: `ws://127.0.0.1:${port}/` },
  },
  webServer: {
    command: "node launch.ts",
    url: `http://127.0.0.1:${port}/`,
    reuseExistingServer: false,
    timeout: 120_000,
    stdout: "pipe",
    stderr: "pipe",
  },
});
