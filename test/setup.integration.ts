/**
 * Integration test setup.
 *
 * Replaces the Tauri invoke() mock with real Rust commands via tauri-test
 * (N-API bridge compiled into src-tauri with `--features tauri-test`).
 *
 * Prerequisites:
 *   1. Run `npm run build:napi` to compile the src-tauri cdylib.
 *   2. Have `jj` and `git` installed and on PATH.
 */

import os from "os";
import path from "path";
import fs from "fs";
import { createRequire } from "node:module";
import { randomUUID } from "crypto";
import { workspaceDiffCoalesce } from "../src/lib/coalesce-in-flight";
import { configure } from "@testing-library/dom";

// Shared DOM polyfills, browser API stubs, Tauri plugin mocks, and hook mocks
import "./setup.common";
import { routeLoopbackSsh } from "./loopback-ssh";
import {
  closeTestPtys,
  setTestEventSource,
  trackPtyInvoke,
} from "./test-event-bus";

// tauri-test invoke runs on spawn_blocking. Kept under the global 10s test
// timeout (vitest.integration.base.ts) so a stuck waitFor reports a clear
// timeout error instead of racing the outer test timeout and showing up as
// a plain, harder-to-diagnose assertion failure.
configure({ asyncUtilTimeout: 8_000 });

// Keep integration tests deterministic: avoid background auto-rebase races
// during commit creation in Rust core.
process.env.TREQ_DISABLE_AUTO_REBASE = "1";

// One app data dir (and so one app treq.db) per worker process.
const testAppDataDir = path.join(
  os.tmpdir(),
  `treq-integration-${process.pid}-${randomUUID()}`,
);
process.env.TREQ_APP_DATA_DIR = testAppDataDir;

const require = createRequire(import.meta.url);
const tauriTest = require("../src-tauri/target") as {
  invoke: (cmd: string, args?: Record<string, unknown>) => Promise<unknown>;
  drainTestEvents: () => { event: string; payload: string }[];
};

setTestEventSource(() => tauriTest.drainTestEvents());

// The desktop app draws its workspace sidebar natively (GPUI); tests drive
// it through test/gpui-sidebar.ts. Its context menus are native too, so they
// are recorded here instead of opened.
vi.mock("../src/lib/native-context-menu", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../src/lib/native-context-menu")>()),
  popupNativeMenu: vi.fn(),
}));
beforeEach(() => {
  // React's terminal pane, which most tests drive; gpui-terminal.test.tsx
  // turns GPUI's on.
  window.__TREQ_REACT_TERMINAL__ = true;
  // React's agent prompt, prompt history and stash dialogs, which most tests
  // drive; gpui-modals.test.tsx turns GPUI's on.
  window.__TREQ_REACT_MODALS__ = true;
  // React's palette and pickers, which most tests drive;
  // gpui-overlays.test.tsx turns GPUI's on.
  window.__TREQ_REACT_OVERLAYS__ = true;
  // Likewise React's remote dialogs; gpui-remote.test.tsx turns GPUI's on.
  window.__TREQ_REACT_REMOTE_DIALOGS__ = true;
  // React's workspace dialogs, which most tests drive;
  // gpui-workspace-dialogs.test.tsx turns GPUI's on.
  window.__TREQ_REACT_WORKSPACE_DIALOGS__ = true;
  // React's remote workspace view, which most remote tests drive;
  // gpui-remote-workspace.test.tsx turns GPUI's on.
  window.__TREQ_REACT_REMOTE_WORKSPACE__ = true;
  window.__TREQ_GPUI_SHELL__ = true;
});
afterEach(() => closeTestPtys(tauriTest.invoke));

// Tracks NAPI invoke() calls still in flight. Some UI polling (e.g. the
// sidebar's workspace-status fetch) isn't cancelled on unmount, so a call
// can still be running against a repo's local.db after its test finishes.
// Deleting the fixture-copy directory (test/utils.tsx) while that call is
// still in flight makes it reject with an "unable to open database file"
// error nothing awaits anymore -- an unhandled rejection. Exposing the
// pending count lets cleanup wait for it to reach zero first.
let pendingInvokes = 0;
let onAllSettled: (() => void) | null = null;

export function waitForPendingInvokes(
  timeoutMs = 4_000,
  quietMs = 75,
): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  let quietSince = pendingInvokes === 0 ? Date.now() : 0;

  return new Promise((resolve) => {
    const finish = () => {
      onAllSettled = null;
      resolve();
    };

    const check = () => {
      if (pendingInvokes === 0) {
        if (!quietSince) quietSince = Date.now();
        if (Date.now() - quietSince >= quietMs) {
          finish();
          return;
        }
      } else {
        quietSince = 0;
      }
      if (Date.now() >= deadline) {
        finish();
        return;
      }
      setTimeout(check, 10);
    };

    onAllSettled = () => {
      quietSince = Date.now();
    };
    check();
  });
}

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn((cmd: string, args?: Record<string, unknown>) => {
    trackPtyInvoke(cmd, args);
    pendingInvokes++;
    const promise = (
      routeLoopbackSsh(tauriTest.invoke, cmd, args ?? {}) ??
      tauriTest.invoke(cmd, args ?? {})
    ).finally(() => {
      pendingInvokes--;
      if (pendingInvokes === 0 && onAllSettled) {
        onAllSettled();
      }
    });
    // Attach a no-op handler so a dropped SWR/effect promise is not an
    // unhandled rejection. Callers that await this same promise still reject.
    void promise.catch(() => {});
    return promise;
  }),
  // The real Rust dispatch behind this harness represents the shipped app's
  // production code path (unlike the screenshot harness, which flips this to
  // false so a browser preview iframe renders for visual QA) -- tests here
  // exercise the real native-webview commands, e.g. openBrowserWebview.
  isTauri: () => true,
}));

afterEach(() => {
  workspaceDiffCoalesce.reset();
});

afterAll(() => {
  try {
    fs.rmSync(testAppDataDir, { recursive: true, force: true });
  } catch {
    // ignore
  }
});
