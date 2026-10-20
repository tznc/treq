import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(here, "../..");

/** gpui-playwright's port (`GPUI_PLAYWRIGHT`). */
export const port = Number(process.env.TREQ_E2E_PORT ?? 9333);

/** The app built with `cargo build --features e2e` (see README.md). */
export const appBinary =
  process.env.TREQ_E2E_BIN ??
  join(
    process.env.CARGO_TARGET_DIR ?? join(repoRoot, "target"),
    "debug",
    "treq",
  );

/** Where `launch.ts` writes the fixture the tests read. */
export const fixtureFile = join(here, ".fixture", "fixture.json");
