// What `launch.ts` and `landing/launch.ts` share: a fresh home and app data
// dir, a fake `claude` agent on PATH, and the `treq` binary built with
// `--features e2e`, started with gpui-playwright listening.
import { execFileSync, spawn } from "node:child_process";
import { chmodSync, mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
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
  const app = spawn(appBinary, [], {
    stdio: "inherit",
    env: {
      ...appEnv(fixture),
      GPUI_PLAYWRIGHT: String(port),
      TREQ_E2E_REPO: fixture.repo,
      ...extra,
    },
  });
  const stop = () => app.kill("SIGTERM");
  process.on("SIGINT", stop);
  process.on("SIGTERM", stop);
  app.on("exit", (code) => process.exit(code ?? 0));
}
