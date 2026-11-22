// What `launch.ts` and `landing/launch.ts` share: a fresh home and app data
// dir, a fake `claude` agent on PATH, and the `treq` binary built with
// `--features e2e`, started with gpui-playwright listening. Also how a spec
// that runs its own app waits for it to start and stops it.
import { execFileSync, spawn, type ChildProcess } from "node:child_process";
import { once } from "node:events";
import { chmodSync, createWriteStream, mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { chromium, expect, type Browser, type Page } from "@playwright/test";
import { appBinary, fixtureFile, port } from "./env.ts";
import type { Fixture } from "./fixture.ts";

export const author = { name: "E2E", email: "e2e@example.com" };

/**
 * Makes the fixture's directories under a new temp dir. The sidebar names
 * the repository after `repoName`.
 */
export function makeFixture(
  prefix: string,
  repoName = "repo",
): Fixture & { bin: string } {
  const root = mkdtempSync(join(tmpdir(), prefix));
  const home = join(root, "home");
  const repo = join(root, repoName);
  const bin = join(root, "bin");
  const agentLog = join(root, "agent-calls.log");
  for (const dir of [home, repo, bin]) mkdirSync(dir, { recursive: true });
  writeFileSync(
    join(home, ".gitconfig"),
    `[user]\n\tname = ${author.name}\n\temail = ${author.email}\n`,
  );
  return { root, home, repo, bin, agentLog };
}

/** The environment the app and its CLI run with. */
export function appEnv(fixture: Fixture & { bin: string }): NodeJS.ProcessEnv {
  const { home, bin } = fixture;
  return {
    ...process.env,
    HOME: home,
    XDG_DATA_HOME: join(home, ".local/share"),
    XDG_CONFIG_HOME: join(home, ".config"),
    XDG_CACHE_HOME: join(home, ".cache"),
    PATH: `${bin}:${process.env.PATH ?? ""}`,
  };
}

/** `git` in `cwd` as the fixture's author. */
export function gitIn(cwd: string, home: string, ...args: string[]): string {
  return execFileSync("git", args, {
    cwd,
    env: {
      ...process.env,
      HOME: home,
      GIT_AUTHOR_NAME: author.name,
      GIT_AUTHOR_EMAIL: author.email,
      GIT_COMMITTER_NAME: author.name,
      GIT_COMMITTER_EMAIL: author.email,
    },
    encoding: "utf8",
    stdio: "pipe",
  });
}

/** Writes an executable script into the fixture's `bin`. */
export function writeBin(bin: string, name: string, script: string): string {
  const path = join(bin, name);
  writeFileSync(path, script);
  chmodSync(path, 0o755);
  return path;
}

/**
 * The agent the app starts: records its arguments and stays open, as an
 * interactive CLI does.
 */
export function writeFakeAgent(fixture: Fixture & { bin: string }): void {
  writeBin(
    fixture.bin,
    "claude",
    `#!/bin/sh\nprintf '%s\\n' "$*" >> '${fixture.agentLog}'\necho "fake agent ready"\nexec cat\n`,
  );
}

/**
 * Writes `fixture.json` for the tests and starts the app on `repo`. The
 * process exits with the app.
 */
export function startApp(
  fixture: Fixture & { bin: string },
  extra: NodeJS.ProcessEnv = {},
): void {
  mkdirSync(join(fixtureFile, ".."), { recursive: true });
  writeFileSync(fixtureFile, JSON.stringify(fixture, null, 2));
  // Echoed as before, and kept in `<root>/app.log` so a spec can look for
  // `panicked` after each step.
  const log = createWriteStream(join(fixture.root, "app.log"));
  const app = spawn(appBinary, [], {
    stdio: ["ignore", "pipe", "pipe"],
    env: {
      ...appEnv(fixture),
      GPUI_PLAYWRIGHT: String(port),
      TREQ_E2E_REPO: fixture.repo,
      ...extra,
    },
  });
  app.stdout?.on("data", (d) => (process.stdout.write(d), log.write(d)));
  app.stderr?.on("data", (d) => (process.stderr.write(d), log.write(d)));
  const stop = () => app.kill("SIGTERM");
  process.on("SIGINT", stop);
  process.on("SIGTERM", stop);
  app.on("exit", (code, signal) => {
    log.write(`\n[harness] app exited: code=${code} signal=${signal}\n`);
    console.log(`[harness] app exited: code=${code} signal=${signal}`);
    process.exit(code ?? 0);
  });
}

function exited(app: ChildProcess): boolean {
  return app.exitCode !== null || app.signalCode !== null;
}

/**
 * Resolves once the app's gpui-playwright server answers on `port`. Fails
 * as soon as `app` exits, else after `timeout`.
 */
export async function serverUp(app: ChildProcess, port: number, timeout = 120_000): Promise<void> {
  await expect(async () => {
    // An exited app ends the wait; it is reported below.
    if (exited(app)) return;
    expect((await fetch(`http://127.0.0.1:${port}/`)).ok, "the server answers").toBe(true);
  }, `the app's Playwright server answers on port ${port}`).toPass({ intervals: [500], timeout });
  if (exited(app)) {
    throw new Error(`the app exited (${app.exitCode ?? app.signalCode}) before its server answered`);
  }
}

/**
 * Connects Playwright to the app on `port` and opens a page on its window.
 * The server can answer before the window is ready for a context, so this
 * retries until `timeout`.
 */
export async function connectApp(
  port: number,
  timeout = 15_000,
): Promise<{ browser: Browser; page: Page }> {
  const connected: { browser: Browser; page: Page }[] = [];
  await expect(async () => {
    const browser = await chromium.connect(`ws://127.0.0.1:${port}/`);
    try {
      connected.push({ browser, page: await (await browser.newContext()).newPage() });
    } catch (error) {
      await browser.close();
      throw error;
    }
  }, `Playwright opens a page on the app on port ${port}`).toPass({ intervals: [1000], timeout });
  const [first] = connected;
  if (!first) throw new Error(`no page on the app on port ${port}`);
  return first;
}

/** Stops `app` with SIGTERM, then SIGKILL if it has not exited after `grace`. */
export async function stopApp(app: ChildProcess, grace = 10_000): Promise<void> {
  if (exited(app)) return;
  const exit = once(app, "exit", { signal: AbortSignal.timeout(grace) });
  app.kill("SIGTERM");
  try {
    await exit;
  } catch (error) {
    if (!(error instanceof Error && error.name === "AbortError")) throw error;
    if (exited(app)) return;
    const killed = once(app, "exit");
    app.kill("SIGKILL");
    await killed;
  }
}
