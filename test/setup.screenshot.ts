/**
 * Screenshot harness setup (scripts/screenshot/specs/**).
 *
 * Same real tauri-test invoke as test/setup.integration.ts, with one
 * difference: it does not fail a run when a still-un-migrated jj_* command
 * gets invoked. test/integration/** enforces zero jj_* calls as an ongoing
 * migration-debt tracker; the screenshot harness exists to show current real
 * behavior (including that debt) rather than gate on it, so it only logs
 * which jj_* commands fired instead of failing the spec.
 *
 * Prerequisites:
 *   1. Run `npm run build:napi` to compile the src-tauri cdylib.
 *   2. Have `jj` and `git` installed and on PATH.
 */

import os from "os";
import path from "path";
import fs from "fs";
import { createRequire } from "node:module";
import { afterAll, afterEach, beforeEach, vi } from "vitest";

import "./setup.common";
import { routeLoopbackSsh } from "./loopback-ssh";
import {
  closeTestPtys,
  setTestEventSource,
  trackPtyInvoke,
} from "./test-event-bus";

process.env.TREQ_DISABLE_AUTO_REBASE = "1";

const testAppDataDir = path.join(os.tmpdir(), `treq-screenshot-${Date.now()}`);
process.env.TREQ_APP_DATA_DIR = testAppDataDir;

const require = createRequire(import.meta.url);
const tauriTest = require("../src-tauri/target") as {
  invoke: (cmd: string, args?: Record<string, unknown>) => Promise<unknown>;
  drainTestEvents: () => { event: string; payload: string }[];
};

setTestEventSource(() => tauriTest.drainTestEvents());
afterEach(() => closeTestPtys(tauriTest.invoke));

// test/utils.tsx imports waitForPendingInvokes from setup.integration.ts, which
// would run that whole setup (a second @tauri-apps/api/core mock among other
// things) inside screenshot runs. Stub it so this file is the only setup.
vi.mock("./setup.integration", () => ({
  waitForPendingInvokes: () => Promise.resolve(),
}));

// The desktop app draws its workspace sidebar natively (GPUI); specs drive it
// through test/gpui-sidebar.ts. Its context menus are native too, so they are
// recorded here instead of opened (same as test/setup.integration.ts).
vi.mock("../src/lib/native-context-menu", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../src/lib/native-context-menu")>()),
  popupNativeMenu: vi.fn(),
}));
beforeEach(() => {
  window.__TREQ_GPUI_SHELL__ = true;
  // React's own Supabase account (same as test/setup.integration.ts).
  window.__TREQ_REACT_SUPABASE__ = true;
});

const jjCalls: string[] = [];

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn((cmd: string, args?: Record<string, unknown>) => {
    trackPtyInvoke(cmd, args);
    // No native shell in jsdom; the call is only recorded for test/gpui-sidebar.ts.
    if (
      cmd === "gpui_sidebar_update" ||
      cmd === "gpui_screen_update" ||
      cmd === "gpui_overlay_update"
    )
      return Promise.resolve();
    if (cmd.startsWith("jj_")) {
      jjCalls.push(cmd);
    }
    return (
      routeLoopbackSsh(tauriTest.invoke, cmd, args ?? {}) ??
      tauriTest.invoke(cmd, args ?? {})
    );
  }),
  convertFileSrc: (filePath: string) => `file://${filePath}`,
  isTauri: () => false,
}));

afterEach(() => {
  if (jjCalls.length > 0) {
    console.log(
      `[app-qa] un-migrated jj_* commands exercised in this spec: ${jjCalls.join(", ")}`,
    );
  }
  jjCalls.length = 0;
});

afterAll(() => {
  try {
    fs.rmSync(testAppDataDir, { recursive: true, force: true });
  } catch {
    // ignore
  }
});
