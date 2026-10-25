// Starts the app for the visual QA run (`npm run qa`): the landing demo
// repository (../landing/seed.ts: stacked workspaces, a real conflict, a
// fake `gh` with PRs, issues and checks, a fake agent) plus what the QA
// shots need on top of it: a supporting repository for the multi-repo
// sidebar, a commit large enough to defer its diff ("Load diff"), and
// uncommitted edits for the Changes tab, and a fake Linear (./fake-linear.ts)
// that a repository reaches once it has a Linear API key. `QA_THEME` (dark|light) picks the
// theme the app starts in.
import { execFileSync } from "node:child_process";
import { copyFileSync, mkdirSync, symlinkSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { fileURLToPath } from "node:url";
import { appBinary } from "../env.ts";
import { gitIn, makeFixture, startApp, writeBin } from "../harness.ts";
import { ALERTING, seedLandingRepo, STACK_PARENT, workspaceDir } from "../landing/seed.ts";
import { startFakeLinear } from "./fake-linear.ts";

const here = dirname(fileURLToPath(import.meta.url));
const fixture = makeFixture("treq-qa-", "event-bus");
const theme = process.env.QA_THEME === "light" ? "light" : "dark";

const fakeGh = join(fixture.root, "fake-gh.mjs");
copyFileSync(join(here, "../landing/fake-gh.mjs"), fakeGh);
writeBin(
  fixture.bin,
  "gh",
  `#!/bin/sh\nexec '${process.execPath}' '${fakeGh}' "$@"\n`,
);
symlinkSync(appBinary, join(fixture.bin, "treq"));

// The agent the terminal pane shows: a short transcript, then a rule as
// wide as the PTY says the terminal is, redrawn in place on every resize
// the way a TUI redraws. A PTY wider than the grid wraps the rule, and the
// redraw leaves its head behind.
writeBin(
  fixture.bin,
  "claude",
  `#!/bin/sh
printf '%s\\n' "$*" >> '${fixture.agentLog}'
printf '\\033[1m> Handle empty event_message in postEvent\\033[0m\\n\\n'
printf '\\033[32m●\\033[0m Read packages/web/src/lib/client.ts\\n'
printf '  \\033[2m└ Updated client.ts (+6 -1)\\033[0m\\n\\n'
rule() {
  cols=$(stty size | cut -d' ' -f2)
  printf '\\r\\033[K\\033[2m%s\\033[0m' "$(printf '%*s' "$cols" '' | tr ' ' '=')"
}
trap rule WINCH
rule
while :; do sleep 0.2; done
`,
);

const { treq } = seedLandingRepo(fixture);
const { repo, home } = fixture;

// A commit whose one file changes more lines than the Commits tab renders
// up front (LARGE_COMMIT_FILE_DIFF_THRESHOLD, 500).
const events = Array.from(
  { length: 640 },
  (_, i) =>
    `  { "id": ${i}, "provider": "${i % 2 ? "slack" : "discord"}", "event_message": "lorem ipsum ${i}" },`,
).join("\n");
const alerting = workspaceDir(repo, ALERTING);
mkdirSync(join(alerting, "fixtures"), { recursive: true });
writeFileSync(join(alerting, "fixtures/events.json"), `[\n${events}\n]\n`);
treq("commit", ALERTING, "-m", "test: add recorded event fixtures");

// Uncommitted edits in the alerting workspace for the Changes tab.
writeFileSync(
  join(alerting, "packages/api/src/alerting.ts"),
  `export function formatAlert(line: string, level = "info"): string {
  return \`[lorem:\${level}] \${line}\`;
}

export function formatError(line: string): string {
  return formatAlert(line, "error");
}
`,
);
writeFileSync(join(alerting, "packages/api/src/levels.ts"), 'export const LEVELS = ["info", "warn", "error"] as const;\n');

// Branches with no workspace yet, for the Open Workspaces dialog: PR #421's,
// #409's and #415's heads, and a plain branch.
for (const branch of ["fix/slack-retry", "docs/ingest-runbook", "feat/spool-readiness", "chore/bump-deps"]) {
  gitIn(repo, home, "branch", branch, "main");
}

// Uncommitted generated files in the stack's base for the Changes tab's
// "Load diff" placeholders: a lockfile (a known generated path) and a file
// `.gitattributes` marks `linguist-generated`, next to a hand-written edit.
const ingest = workspaceDir(repo, STACK_PARENT);
writeFileSync(join(ingest, ".gitattributes"), "packages/api/src/generated/** linguist-generated\n");
writeFileSync(
  join(ingest, "package-lock.json"),
  `${JSON.stringify({ name: "event-bus", lockfileVersion: 3, packages: { "": { name: "event-bus" } } }, null, 2)}\n`,
);
mkdirSync(join(ingest, "packages/api/src/generated"), { recursive: true });
writeFileSync(
  join(ingest, "packages/api/src/generated/schema.ts"),
  '// Generated from schema.json. Do not edit.\nexport const EVENT_FIELDS = ["event_message", "provider", "payload"] as const;\n',
);
writeFileSync(join(ingest, "packages/api/src/retry.ts"), "export const MAX_RETRIES = 3;\n");

// A supporting repository: the sidebar's second repo.
const docs = join(fixture.root, "docs-site");
mkdirSync(docs, { recursive: true });
const git = (...args: string[]) => gitIn(docs, home, ...args);
git("init", "-b", "main");
writeFileSync(join(docs, "README.md"), "# docs-site\n\nLorem ipsum docs.\n");
writeFileSync(join(docs, ".gitignore"), ".jj/\n.treq/\n");
git("add", ".");
git("commit", "-m", "docs: start the docs site");
try {
  execFileSync(appBinary, ["add", "docs/getting-started"], {
    cwd: docs,
    env: { ...process.env, HOME: home, PATH: `${fixture.bin}:${process.env.PATH}` },
    stdio: "pipe",
  });
} catch (error) {
  console.error("qa: could not add a workspace to docs-site", error);
}
const local = new DatabaseSync(join(repo, ".treq", "local.db"));
local.exec(
  "CREATE TABLE IF NOT EXISTS supporting_repos (id INTEGER PRIMARY KEY AUTOINCREMENT, repo_path TEXT NOT NULL UNIQUE, added_at TEXT NOT NULL)",
);
local
  .prepare(
    "INSERT OR IGNORE INTO supporting_repos (repo_path, added_at) VALUES (?, ?)",
  )
  .run(docs, new Date().toISOString());
local.close();

const appDb = new DatabaseSync(join(home, ".local/share/com.treq/treq.db"));
appDb
  .prepare("INSERT OR REPLACE INTO settings (key, value) VALUES ('theme', ?)")
  .run(theme);
appDb.close();

writeFileSync(join(fixture.root, "qa-extra.json"), JSON.stringify({ docs, theme, repo }));

const linearPort = Number(process.env.TREQ_E2E_PORT ?? 9333) + 2;
startFakeLinear(linearPort);

startApp(fixture, {
  GDK_SCALE: process.env.GDK_SCALE ?? "2",
  TREQ_E2E_LINEAR_URL: `http://127.0.0.1:${linearPort}/graphql`,
  TREQ_FAKE_GH_LOG: join(fixture.root, "gh-calls.log"),
});
