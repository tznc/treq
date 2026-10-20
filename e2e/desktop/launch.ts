// Starts the desktop app for the suite: a fresh home and app data dir, a
// git repository to open, and a fake `claude` agent on PATH, then the
// `treq` binary built with `--features e2e`. Playwright's `webServer` runs
// it and waits for gpui-playwright's port; the tests read the fixture from
// `fixture.json` (see `fixture.ts`).
import { spawn, execFileSync } from "node:child_process";
import { chmodSync, mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { appBinary, fixtureFile, port } from "./env.ts";

const root = mkdtempSync(join(tmpdir(), "treq-e2e-"));
const home = join(root, "home");
const repo = join(root, "repo");
const bin = join(root, "bin");
const agentLog = join(root, "agent-calls.log");
for (const dir of [home, repo, bin]) mkdirSync(dir, { recursive: true });

const git = (...args: string[]) =>
  execFileSync("git", args, {
    cwd: repo,
    env: {
      ...process.env,
      HOME: home,
      GIT_AUTHOR_NAME: "E2E",
      GIT_AUTHOR_EMAIL: "e2e@example.com",
      GIT_COMMITTER_NAME: "E2E",
      GIT_COMMITTER_EMAIL: "e2e@example.com",
    },
    stdio: "pipe",
  });
git("init", "-b", "main");
writeFileSync(join(repo, "README.md"), "# e2e fixture\n");
writeFileSync(join(repo, "app.txt"), "first line\n");
git("add", ".");
git("commit", "-m", "Initial commit");

// The agent the app starts: records its arguments and stays open, as an
// interactive CLI does.
const agent = join(bin, "claude");
writeFileSync(
  agent,
  `#!/bin/sh\nprintf '%s\\n' "$*" >> '${agentLog}'\necho "fake agent ready"\nexec cat\n`,
);
chmodSync(agent, 0o755);

writeFileSync(
  join(home, ".gitconfig"),
  "[user]\n\tname = E2E\n\temail = e2e@example.com\n",
);

mkdirSync(join(fixtureFile, ".."), { recursive: true });
writeFileSync(
  fixtureFile,
  JSON.stringify({ root, home, repo, agentLog }, null, 2),
);

const app = spawn(appBinary, [], {
  stdio: "inherit",
  env: {
    ...process.env,
    HOME: home,
    XDG_DATA_HOME: join(home, ".local/share"),
    XDG_CONFIG_HOME: join(home, ".config"),
    XDG_CACHE_HOME: join(home, ".cache"),
    PATH: `${bin}:${process.env.PATH ?? ""}`,
    GPUI_PLAYWRIGHT: String(port),
    TREQ_E2E_REPO: repo,
  },
});
const stop = () => app.kill("SIGTERM");
process.on("SIGINT", stop);
process.on("SIGTERM", stop);
app.on("exit", (code) => process.exit(code ?? 0));
