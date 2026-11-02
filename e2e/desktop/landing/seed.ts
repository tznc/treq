// The demo repository the landing screenshots show: a lorem-ipsum event-bus
// with stacked workspaces, two siblings, and a real conflict under
// packages/web (the parent rewrote Home.tsx after the child forked).
// Workspaces and commits go through the `treq` CLI, so the app opens on the
// same database a user's repository would have.
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { appBinary } from "../env.ts";
import { appEnv, gitIn, type makeFixture } from "../harness.ts";

export const STACK_PARENT = "feat/event-ingest";
export const MARKETING = "feat/empty-event-message";
export const CACHE = "feat/keyvalues-cache";
export const ALERTING = "feat/alerting-logs";

const README = `# event-bus

Lorem ipsum dolor sit amet, consectetur adipiscing elit. The ingest
pipeline accepts Discord and Slack webhooks, normalizes them into
\`EventBody\`, and fans them out to alerting and the web feed.

## Packages

- \`packages/api\` — ingest, schema, retries
- \`packages/web\` — event feed and client
`;

const INGEST_SRC = `export type Provider = "discord" | "slack";

export async function ingest(provider: Provider, raw: unknown): Promise<void> {
  const body = JSON.stringify(raw);
  await fetch("/events", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body,
  });
}
`;

const SCHEMA_SRC = `export type EventBody = {
  event_message?: string;
  provider: "discord" | "slack";
  payload: Record<string, unknown>;
};
`;

const CLIENT_SRC = `import type { EventBody } from "../../../api/src/schema";

export async function postEvent(body: EventBody): Promise<Response> {
  const message =
    typeof body.event_message === "string" && body.event_message.length > 0
      ? body.event_message
      : JSON.stringify(body);

  return fetch("/events", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ ...body, event_message: message }),
  });
}
`;

const CLIENT_TEST = `import { describe, expect, it, vi } from "vitest";
import { postEvent } from "./client";

describe("postEvent", () => {
  it("json stringifies body when event_message is absent", async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValue(new Response(null, { status: 204 }));
    vi.stubGlobal("fetch", fetchMock);

    await postEvent({
      provider: "discord",
      payload: { id: 1 },
    });

    expect(fetchMock).toHaveBeenCalledWith(
      "/events",
      expect.objectContaining({
        method: "POST",
        body: expect.stringContaining('"event_message"'),
      }),
    );
  });
});
`;

const HOME_MAIN = `export function HomePage() {
  return (
    <main>
      <h1>Lorem ipsum dolor sit amet</h1>
      <p>Consectetur adipiscing elit. Sed do eiusmod tempor incididunt.</p>
    </main>
  );
}
`;

const HOME_CHILD = `import { EventFeed } from "../components/EventFeed";

export function HomePage() {
  return (
    <main>
      <h1>Lorem ipsum — event feed</h1>
      <EventFeed source="discord" />
    </main>
  );
}
`;

const HOME_PARENT_LATER = `export function HomePage() {
  return (
    <main>
      <h1>Lorem ipsum dolor sit amet</h1>
      <p>Ut labore et dolore magna aliqua. The ingest stack owns this page.</p>
    </main>
  );
}
`;

const EVENT_FEED = `export function EventFeed(props: { source: "discord" | "slack" }) {
  return <section aria-label="event feed">{props.source}</section>;
}
`;

const CACHE_SRC = `const store = new Map<string, string>();

export function setKey(key: string, value: string): void {
  store.set(key, value);
}

export function getKey(key: string): string | undefined {
  return store.get(key);
}
`;

const CACHE_TEST = `import { getKey } from "./cache";

export function miss(): string {
  return getKey("missing") ?? "";
}
`;

const ALERTING_SRC = `export function formatAlert(line: string): string {
  return \`[lorem] \${line}\`;
}
`;

/** The uncommitted edits the isolation and review shots make. */
export const CACHE_EDIT = CACHE_SRC.replace(
  "const store",
  "export const CACHE_TTL = 30;\n\nconst store",
);
export const FEED_SRC = 'export const FEED_SOURCE = "discord";\n';

type Fixture = ReturnType<typeof makeFixture>;

/** A workspace's checkout: `.treq/workspaces/<branch with / as ->`. */
export function workspaceDir(repo: string, branch: string): string {
  // A new workspace's directory starts with its id: `<id>-<branch>`.
  const root = join(repo, ".treq", "workspaces");
  const name = branch.replaceAll("/", "-");
  const dirs = existsSync(root) ? readdirSync(root) : [];
  const found = dirs.find(
    (d) => d === name || (d.endsWith(`-${name}`) && /^\d+$/.test(d.slice(0, -name.length - 1))),
  );
  return join(root, found ?? name);
}

/** Writes `file` in a workspace's checkout, as an editor would. */
export function writeInWorkspace(
  repo: string,
  branch: string,
  file: string,
  text: string,
): void {
  write(workspaceDir(repo, branch), file, text);
}

function write(dir: string, file: string, text: string): void {
  const path = join(dir, file);
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, text);
}

/** Seeds `fixture.repo`; returns the `treq` CLI runner for later edits. */
export function seedLandingRepo(fixture: Fixture) {
  const { repo, home } = fixture;
  const git = (...args: string[]) => gitIn(repo, home, ...args);
  const appData = join(home, ".local/share/com.treq");
  const treq = (...args: string[]) =>
    execFileSync(appBinary, args, {
      cwd: repo,
      // The app's own data dir, which it exports to the CLIs it runs.
      env: { ...appEnv(fixture), TREQ_APP_DATA_DIR: appData },
      encoding: "utf8",
      stdio: "pipe",
    });
  const commitIn = (
    branch: string,
    files: Record<string, string>,
    msg: string,
  ) => {
    for (const [file, text] of Object.entries(files)) {
      write(workspaceDir(repo, branch), file, text);
    }
    treq("commit", branch, "-m", msg);
  };

  git("init", "-b", "main");
  git(
    "remote",
    "add",
    "origin",
    "https://github.com/lorem-ipsum/event-bus.git",
  );
  write(repo, "README.md", README);
  // What `treq add` would add on first use, committed up front so main
  // does not grow a .gitignore commit the workspaces miss.
  // The rest are the heavy and local paths a real repo ignores; the create
  // dialog offers each as a symlink suggestion.
  write(
    repo,
    ".gitignore",
    ".jj/\n.treq/\nnode_modules/\ndist/\ncoverage/\n.turbo/\n.env.local\nCLAUDE.local.md\ntsconfig.tsbuildinfo\n",
  );
  git("add", ".");
  git("commit", "-m", "docs: describe event-bus");
  write(repo, "packages/web/src/pages/Home.tsx", HOME_MAIN);
  git("add", ".");
  git("commit", "-m", "feat: add lorem home page");
  write(repo, "packages/web/src/components/EventFeed.tsx", EVENT_FEED);
  git("add", ".");
  git("commit", "-m", "feat: add event feed stub");

  for (const branch of [ALERTING, CACHE, STACK_PARENT]) treq("add", branch);
  commitIn(
    STACK_PARENT,
    { "packages/api/src/ingest.ts": INGEST_SRC },
    "feat: ingest Discord and Slack events",
  );
  commitIn(
    STACK_PARENT,
    { "packages/api/src/schema.ts": SCHEMA_SRC },
    "feat: add EventBody schema",
  );

  treq("add", MARKETING, "-s", STACK_PARENT);
  commitIn(
    MARKETING,
    { "README.md": README, "packages/web/src/lib/client.ts": CLIENT_SRC },
    "feat: handle empty event messages",
  );
  commitIn(
    MARKETING,
    {
      "packages/web/src/lib/client.test.ts": CLIENT_TEST,
      "packages/web/src/pages/Home.tsx": HOME_CHILD,
    },
    "feat: wire event feed on the home page",
  );

  commitIn(
    CACHE,
    { "packages/api/src/cache.ts": CACHE_SRC },
    "feat: keyvalues cache",
  );
  commitIn(
    CACHE,
    { "packages/api/src/cache.test.ts": CACHE_TEST },
    "test cache misses",
  );
  commitIn(
    ALERTING,
    { "packages/api/src/alerting.ts": ALERTING_SRC },
    "feat: alerting logs",
  );

  // The parent diverges after the child forked: both rewrite the same lines
  // of Home.tsx, so the child's rebase conflicts.
  commitIn(
    STACK_PARENT,
    { "packages/web/src/pages/Home.tsx": HOME_PARENT_LATER },
    "fix: clarify home copy for ingest owners",
  );

  // The capture shows the dark theme, as the site does.
  mkdirSync(appData, { recursive: true });
  const db = new DatabaseSync(join(appData, "treq.db"));
  db.exec(
    "CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT NOT NULL)",
  );
  db.prepare(
    "INSERT OR REPLACE INTO settings (key, value) VALUES ('theme', 'dark')",
  ).run();
  db.close();

  return { treq, git };
}
