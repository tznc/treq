import { defineConfig } from "vitest/config";

/**
 * The Supabase-side unit tests: billing, Linear proxy and the
 * remote SSH relay's traceability. The desktop app's tests are Rust
 * (`cargo test`); `scripts/service-qa` has its own config.
 */
export default defineConfig({
  // test/web imports web/src, whose tsconfig extends a package only web/ installs.
  esbuild: { tsconfigRaw: "{}" },
  test: {
    environment: "node",
    globals: true,
    include: [
      "test/billing/**/*.test.ts",
      "test/account/**/*.test.ts",
      "test/linear-proxy/**/*.test.ts",
      "test/desktop-token/**/*.test.ts",
      "test/github/**/*.test.ts",
      "test/edge-http/**/*.test.ts",
      "test/remote-ssh-traceability.test.ts",
      "test/web/**/*.test.ts",
    ],
    testTimeout: 15000,
  },
});
