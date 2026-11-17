// e2e/mobile: the mobile app's desktop build (`--example mobile --features
// mobile-e2e`) driven through gpui-playwright against a real SSH host, as
// qa/18-ssh-remote.spec.ts drives the desktop app. Each spec starts its own
// app; see mobile/README.md.
import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: ".",
  testMatch: /.*\.spec\.ts/,
  workers: 1,
  fullyParallel: false,
  forbidOnly: !!process.env.CI,
  timeout: 180_000,
  reporter: "list",
  expect: { timeout: 30_000 },
});
