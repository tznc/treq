// Remote SSH against a real server: a second app with a fresh home whose
// ~/.ssh holds the key the server trusts, driven through the Overview
// screen's machines, the SSH Hosts +, the repository form, New
// Workspaces, a remote shell, installing Claude Code, Codex, Cursor and Copilot
// with their installers and starting each as an agent, and a dropped connection. Opt-in: set QA_SSH_HOST, QA_SSH_PORT, QA_SSH_USER,
// QA_SSH_KEY (a private key with its `.pub` beside it) and QA_SSH_REPO (a
// path on the server the run replaces with a fresh Git repository). The
// server needs `treq`, jj, git, tmux and curl, and must reach
// claude.ai, cursor.com, nodejs.org and registry.npmjs.org;
// `ssh-server/up.sh` starts one in Docker and prints the
// variables.
import { execFileSync, spawn, type ChildProcess } from "node:child_process";
import { copyFileSync, mkdirSync } from "node:fs";
import { join } from "node:path";
import { chromium, expect, test, type Browser, type Page } from "@playwright/test";
import { appBinary, port } from "../env.ts";
import { appEnv, makeFixture } from "../harness.ts";
import { capture, theme } from "./capture.ts";

const env = {
  host: process.env.QA_SSH_HOST ?? "",
  port: process.env.QA_SSH_PORT ?? "22",
  user: process.env.QA_SSH_USER ?? "",
  key: process.env.QA_SSH_KEY ?? "",
  repo: process.env.QA_SSH_REPO ?? "",
};
// The sidebar names a workspace by its title, not its branch.
const WORKSPACE = "Remote feature";
const sshPort = port + 4;
let app: ChildProcess;
let browser: Browser;
let page: Page;

test.describe.configure({ mode: "serial" });
test.skip(!env.host, "QA_SSH_HOST is not set");

/** Runs `script` on the server, outside the app. */
function onServer(script: string): string {
  return execFileSync(
    "ssh",
    [
      "-i", env.key, "-p", env.port,
      "-o", "StrictHostKeyChecking=no", "-o", "UserKnownHostsFile=/dev/null", "-o", "LogLevel=ERROR",
      `${env.user}@${env.host}`, script,
    ],
    { encoding: "utf8" },
  );
}

test.beforeAll(async () => {
  // A plain Git repository, as most servers have: no jj, no .treq.
  onServer(
    `set -e; rm -rf '${env.repo}'; mkdir -p '${env.repo}'; cd '${env.repo}'
     git init -q -b main
     git config user.name QA; git config user.email qa@example.com
     printf '# demo\\n' > README.md; mkdir src
     printf 'fn main() {}\\n' > src/main.rs
     git add .; git commit -qm 'Initial commit'
     printf 'pub fn add(a: i32, b: i32) -> i32 { a + b }\\n' > src/lib.rs
     git add .; git commit -qm 'Add lib'
     tmux kill-server 2>/dev/null || true`,
  );
  const fixture = makeFixture("treq-qa-ssh-");
  const ssh = join(fixture.home, ".ssh");
  mkdirSync(ssh, { recursive: true, mode: 0o700 });
  copyFileSync(env.key, join(ssh, "id_ed25519"));
  copyFileSync(`${env.key}.pub`, join(ssh, "id_ed25519.pub"));
  const { DatabaseSync } = await import("node:sqlite");
  const dir = join(fixture.home, ".local/share/com.treq");
  mkdirSync(dir, { recursive: true });
  const db = new DatabaseSync(join(dir, "treq.db"));
  db.exec("CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT NOT NULL)");
  db.prepare("INSERT OR REPLACE INTO settings (key, value) VALUES ('theme', ?)").run(theme);
  db.close();
  app = spawn(appBinary, [], {
    stdio: process.env.QA_DEBUG ? "inherit" : "ignore",
    env: { ...appEnv(fixture), GPUI_PLAYWRIGHT: String(sshPort), GDK_SCALE: process.env.GDK_SCALE ?? "2" },
  });
  const deadline = Date.now() + 120_000;
  for (;;) {
    try {
      if ((await fetch(`http://127.0.0.1:${sshPort}/`)).ok) break;
    } catch {}
    if (Date.now() > deadline) throw new Error("ssh app did not start");
    await new Promise((r) => setTimeout(r, 500));
  }
  browser = await chromium.connect(`ws://127.0.0.1:${sshPort}/`);
  page = await (await browser.newContext()).newPage();
  await page.setViewportSize({ width: 1400, height: 900 });
});

test.afterAll(async () => {
  await browser?.close().catch(() => {});
  app?.kill("SIGTERM");
});

async function shot(name: string, title: string, expectations: string[]): Promise<void> {
  await page.waitForTimeout(1500);
  if (process.env.QA_DEBUG) console.log(`--- ${name}\n${await page.ariaSnapshot()}`);
  await capture(page, { name: `18-ssh-${name}`, title, screen: true, expectations });
}

test("ssh remote: connect and open a Git repository", async () => {
  const open = page.getByRole("button", { name: "Add SSH host" });
  await expect(open).toBeVisible({ timeout: 60_000 });
  await expect(page.getByRole("button", { name: "Sign in to Treq Cloud" })).toBeVisible();
  await expect(page.getByRole("listitem", { name: "Cloud workspace", exact: true })).toHaveCount(0);
  await shot("machines", "Overview: machines and recent repositories", [
    "Left panel 'Machines' lists 'This computer' under THIS COMPUTER, then SSH HOSTS ('No hosts yet.') and DEV CONTAINERS, each with a + beside its heading",
    "Signed out there is no TREQ CLOUD group; 'Sign in to Treq Cloud' sits at the bottom",
    "Right panel is headed 'Overview' with the Open repository… button; no recents yet says where they appear",
  ]);
  await page.getByRole("button", { name: /on This computer$/ }).click();
  await shot("local-tools", "This computer's tools popover", [
    "A popover beside the row lists Required (Git), Agent CLI and Optional tools with Installed or Install",
  ]);
  await page.keyboard.press("Escape");
  // The + next to SSH Hosts opens the own-VM form directly.
  await open.click();
  await page.waitForTimeout(1500);
  // Display name, hostname, port, username: the form's text fields in order.
  const fields = page.getByRole("textbox");
  await fields.nth(0).fill("QA container");
  await fields.nth(1).fill(env.host);
  await fields.nth(2).fill(env.port);
  await fields.nth(3).fill(env.user);
  await shot("own-vm", "Your own VM form, filled in", [
    "The SSH key select shows id_ed25519 (ssh-ed25519) from ~/.ssh",
    "Continue is enabled",
  ]);
  await page.getByRole("button", { name: "Continue" }).click();
  await expect(page.getByRole("button", { name: "Trust and connect" })).toBeVisible({ timeout: 30_000 });
  await shot("trust", "Confirm host trust", [
    "Names user@host:port and the server's SHA256 host key fingerprint (compare with ssh-keygen -lf on the server)",
  ]);
  await page.getByRole("button", { name: "Trust and connect" }).click();
  const repoName = env.repo.split("/").pop()!;
  const repoFolder = page.getByRole("listitem", { name: repoName, exact: true });
  await expect(repoFolder).toBeVisible({ timeout: 30_000 });
  await shot("connected", "Connected: browsing the host from its home", [
    "'QA container' is listed under SSH HOSTS with user@host:port",
    "Its group on the right browses /home/dev: breadcrumbs, Show hidden (off) and the folders, with qa-demo marked Repository",
    "No dot folders while Show hidden is off; the plain home folder offers Use this folder",
  ]);
  await page.getByRole("checkbox", { name: "Show hidden folders" }).click();
  await expect(page.getByRole("listitem", { name: ".ssh", exact: true })).toBeVisible({ timeout: 30_000 });
  await shot("hidden", "Show hidden folders", ["Dot folders such as .ssh now show"]);
  await page.getByRole("checkbox", { name: "Show hidden folders" }).click();
  await page.getByRole("button", { name: /on QA container$/ }).click();
  await expect(page.getByRole("button", { name: "Remove host…" })).toBeVisible({ timeout: 30_000 });
  await page.waitForTimeout(3000);
  await shot("host-tools", "The SSH host's tools popover", [
    "Asked over SSH (`treq repo tools`): Git, Jujutsu and tmux are installed",
    "Claude Code is not installed yet; Remove host… at the bottom",
  ]);
  await page.keyboard.press("Escape");
  await repoFolder.click();
  const openRemote = page.getByRole("button", { name: "Open", exact: true });
  await expect(openRemote).toBeVisible({ timeout: 30_000 });
  await shot("probed", "Browsed into the repository", [
    "The breadcrumbs end at qa-demo and Open shows for the repository",
  ]);
  await openRemote.click();
  await expect(page.getByRole("tab", { name: "Commits" })).toBeVisible({ timeout: 60_000 });
  await page.getByRole("tab", { name: "Commits" }).click();
  await page.waitForTimeout(3000);
  await expect(page.getByRole("status", { name: "Couldn't load commits" })).toHaveCount(0);
  await shot("commits", "Remote repository: Commits", [
    "The repository path and Terminal/Refresh links head the window",
    "'Add lib' and 'Initial commit' are listed: jj was set up on the plain Git repository",
  ]);
});

test("ssh remote: New Workspaces creates on the server", async () => {
  await page.getByRole("button", { name: "New workspaces" }).click();
  await expect(page.getByRole("heading", { name: "New Workspaces" })).toBeVisible();
  await page.getByLabel("Workspaces", { exact: true }).fill("Remote feature");
  await expect(page.getByRole("button", { name: "Create 1 Workspace" })).toBeEnabled({ timeout: 20_000 });
  await page.getByRole("button", { name: "Create 1 Workspace" }).click();
  const row = page.getByRole("button", { name: WORKSPACE, exact: true });
  await expect(row).toBeVisible({ timeout: 60_000 });
  await expect(page.getByRole("heading", { name: "New Workspaces" })).toHaveCount(0);
  await shot("workspace-created", "Remote workspace created", [
    `${WORKSPACE} shows under WORKSPACES; the dialog closed with no error`,
  ]);
  expect(onServer(`ls '${env.repo}/.treq/workspaces'`)).toContain("remote-feature");
});

test("ssh remote: shell, Changes, Commits and agent", async () => {
  const row = page.getByRole("button", { name: WORKSPACE, exact: true });
  await row.click();
  await row.hover();
  await row.getByRole("button", { name: "Open shell" }).click();
  await page.waitForTimeout(4000);
  await page.mouse.click(700, 780);
  await page.keyboard.type(
    "hostname; pwd; printf 'pub fn sub(a: i32, b: i32) -> i32 { a - b }\\n' >> src/lib.rs; echo notes > NOTES.md\n",
  );
  await page.waitForTimeout(2000);
  await shot("shell", "Remote shell in the workspace", [
    "The prompt and output come from the server: its hostname and the workspace's path under .treq/workspaces",
    "No tmux status line at the bottom of the pane",
  ]);
  // Away to Settings and back: the same remote shell, still attached.
  await page.getByRole("button", { name: "Settings", exact: true }).click();
  await expect(page.getByRole("button", { name: "Close", exact: true })).toBeVisible();
  await page.waitForTimeout(1500);
  await row.click();
  await page.waitForTimeout(3000);
  await page.mouse.click(700, 780);
  await page.keyboard.type("echo back from settings\n");
  await page.waitForTimeout(2000);
  await shot("shell-after-settings", "The remote shell after a trip to Settings", [
    "The same shell column shows the hostname and pwd output, then 'back from settings'",
    "No Reattach overlay: the channel stayed open while Settings showed",
  ]);
  await page.getByRole("button", { name: "Refresh" }).click();
  await page.getByRole("tab", { name: "Changes" }).click();
  await page.waitForTimeout(3000);
  await shot("changes", "Remote workspace: Changes", [
    "NOTES.md (A) and lib.rs (M) are listed with their diffs",
  ]);
  await page.getByRole("textbox", { name: "Message" }).fill("Add sub");
  await page.getByRole("button", { name: "Commit", exact: true }).click();
  await page.waitForTimeout(4000);
  await page.getByRole("tab", { name: "Commits" }).click();
  await page.waitForTimeout(3000);
  expect(onServer(`cd '${env.repo}/.treq/workspaces/'*/ && jj log --no-graph -r @- -T description`)).toContain("Add sub");
  await shot("ws-commits", "Remote workspace: committed", ["'Add sub' is the workspace's commit"]);
});

/** Picks the agent at `row` in Settings' Default Agent select and saves. */
async function setDefaultAgent(row: number): Promise<void> {
  await page.getByRole("button", { name: "Settings", exact: true }).click();
  // The application's agent: a remote repository's setting is not read.
  await page.getByRole("tab", { name: "Application" }).click();
  await page.waitForTimeout(800);
  const select = page.getByRole("combobox", { name: "Default Agent" });
  await select.scrollIntoViewIfNeeded();
  const box = await select.boundingBox();
  if (!box) throw new Error("no Default Agent select");
  await select.click();
  await page.waitForTimeout(800);
  // Options: Default, Claude, Codex, Cursor, Copilot.
  await page.mouse.click(box.x + 40, box.y + box.height + 26 + row * 30);
  await page.waitForTimeout(500);
  await page.getByRole("button", { name: "Save Settings" }).click();
  await page.waitForTimeout(1000);
  await page.getByRole("button", { name: "Close", exact: true }).click();
}

// The four agent CLIs Treq starts, each with its provider's installer.
test("ssh remote: install the agent CLIs", async () => {
  test.setTimeout(900_000);
  onServer(
    `set -e; cd ~; mkdir -p ~/.local/bin
     curl -fsSL https://claude.ai/install.sh | bash >/dev/null 2>&1
     curl -fsS https://cursor.com/install | bash >/dev/null 2>&1
     v=$(curl -fsS https://nodejs.org/dist/index.json | grep -o '"v22[.0-9]*"' | head -1 | tr -d '"')
     curl -fsSL https://nodejs.org/dist/$v/node-$v-linux-x64.tar.gz | tar xz -C ~
     for b in node npm npx; do ln -sf ~/node-$v-linux-x64/bin/$b ~/.local/bin/$b; done
     export PATH=~/.local/bin:$PATH; npm config set prefix ~/.local
     npm i -g @openai/codex @github/copilot >/dev/null 2>&1`,
  );
  for (const bin of ["claude", "codex", "cursor-agent", "copilot"]) {
    // A login shell, as Treq starts agents: npm's CLIs need node on PATH.
    expect(onServer(`bash -lc '${bin} --version'`)).not.toBe("");
  }
});

// Each agent starts from the workspace's Start agent button, with no
// credentials on the host, and stops at its provider's first-run screen.
for (const [label, row, bin] of [
  ["Claude", 1, "claude"],
  ["Codex", 2, "codex"],
  ["Cursor", 3, "cursor-agent"],
  ["Copilot", 4, "copilot"],
] as const) {
  test(`ssh remote: start ${label} and send it input`, async () => {
    // One agent at a time, so its pane has the full width.
    for (const stop of await page.getByRole("button", { name: "Stop", exact: true }).all()) await stop.click();
    await setDefaultAgent(row);
    const workspace = page.getByRole("button", { name: WORKSPACE, exact: true });
    await workspace.click();
    await workspace.hover();
    await workspace.getByRole("button", { name: "Start agent" }).click();
    await page.waitForTimeout(12_000);
    await shot(`agent-${bin}`, `${label}: the real CLI's first-run screen`, [
      `The agent pane, titled '${bin.replace("-", "_")}-…', shows ${label}'s own welcome or sign-in screen`,
    ]);
    await page.mouse.click(700, 780);
    await page.keyboard.press("ArrowDown");
    await page.keyboard.type("hello");
    await page.waitForTimeout(3000);
    await shot(`agent-${bin}-input`, `${label} after typed input`, [
      "The screen reacts to the keys: a moved selection, typed text, or the next first-run step",
    ]);
  });
}

test("ssh remote: install Claude Code and run it as an agent", async () => {
  test.setTimeout(600_000);
  const row = page.getByRole("button", { name: WORKSPACE, exact: true });
  // The official installer, typed into the remote shell as a user would.
  await page.mouse.click(700, 780);
  await page.keyboard.type("clear; curl -fsSL https://claude.ai/install.sh | bash\n");
  const deadline = Date.now() + 480_000;
  while (!onServer("test -x ~/.local/bin/claude && echo yes || true").includes("yes")) {
    if (Date.now() > deadline) throw new Error("Claude Code did not install");
    await page.waitForTimeout(5000);
  }
  await page.waitForTimeout(5000);
  await page.keyboard.type("~/.local/bin/claude --version\n");
  await shot("claude-installed", "Claude Code installed on the server", [
    "The installer's output ends with 'Installation complete!' in the remote shell",
    "`claude --version` prints the installed version",
  ]);
  await row.hover();
  await row.getByRole("button", { name: "Start agent" }).click();
  await page.waitForTimeout(10_000);
  await shot("agent", "Claude Code running as a remote agent", [
    "The agent pane runs the real Claude Code TUI on the server, in the workspace directory",
    "Without credentials it stops at its first-run sign-in screen",
  ]);
});

test("ssh remote: dropped connection reattaches on its own", async () => {
  // Detached, so killing this command's own connection does not stop it
  // half way through.
  onServer(`setsid sh -c "sleep 1; pkill -f 'sshd: .*@'" </dev/null >/dev/null 2>&1 &`);
  await page.waitForTimeout(10_000);
  await expect(page.getByRole("button", { name: "Reattach" })).toHaveCount(0);
  await shot("reattached", "After the server drops every SSH connection", [
    "Every remote terminal reattached on its own, with no Reattach/Close overlay",
    "The shell and agent show their earlier output: the tmux sessions survived on the server",
  ]);
  await page.getByRole("button", { name: "Refresh" }).click();
  await page.getByRole("tab", { name: "Changes" }).click();
  await shot("refreshed", "Refresh after the drop", ["The tab reloads over a new connection with no error"]);
});
