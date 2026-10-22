// Remote SSH against a real server: a second app with a fresh home whose
// ~/.ssh holds the key the server trusts, driven through "Your own VM",
// the repository form, New Workspaces, a remote shell and agent, and a
// dropped connection. Opt-in: set QA_SSH_HOST, QA_SSH_PORT, QA_SSH_USER,
// QA_SSH_KEY (a private key with its `.pub` beside it) and QA_SSH_REPO (a
// path on the server the run replaces with a fresh Git repository). The
// server needs `treq`, jj, git, tmux and a `claude` on the login PATH;
// `ssh-server/up.sh` starts one in Docker and prints the variables.
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
const BRANCH = "treq/remote-feature";
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
  const open = page.getByRole("button", { name: "Open via SSH" });
  await expect(open).toBeVisible({ timeout: 60_000 });
  await open.click();
  await page.waitForTimeout(1000);
  // The chooser's cards are not observable elements (see 01-onboarding).
  await page.mouse.click(834, 478);
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
  const path = page.getByRole("textbox").first();
  await expect(page.getByRole("button", { name: "Probe" })).toBeVisible({ timeout: 30_000 });
  await shot("connected", "Connected: the remote repository form", [
    "An 'Open a Remote Repository' card is centred where the window opens, above the local card",
    "Its path field and Probe button are visible without scrolling",
  ]);
  await path.fill(env.repo);
  await page.getByRole("button", { name: "Probe" }).click();
  const openRemote = page.getByRole("button", { name: "Open", exact: true });
  await expect(openRemote).toBeVisible({ timeout: 30_000 });
  await shot("probed", "Remote repository probed", ["The probe found a repository and offers Open"]);
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
  const row = page.getByRole("button", { name: BRANCH, exact: true });
  await expect(row).toBeVisible({ timeout: 60_000 });
  await expect(page.getByRole("heading", { name: "New Workspaces" })).toHaveCount(0);
  await shot("workspace-created", "Remote workspace created", [
    `${BRANCH} shows under WORKSPACES; the dialog closed with no error`,
  ]);
  expect(onServer(`ls '${env.repo}/.treq/workspaces'`)).toContain("remote-feature");
});

test("ssh remote: shell, Changes, Commits and agent", async () => {
  const row = page.getByRole("button", { name: BRANCH, exact: true });
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
  await row.hover();
  await row.getByRole("button", { name: "Start agent" }).click();
  await page.waitForTimeout(5000);
  await shot("agent", "Remote agent", [
    "The agent pane runs `claude` on the server in the workspace directory",
  ]);
});

test("ssh remote: dropped connection and reattach", async () => {
  // Detached, so killing this command's own connection does not stop it
  // half way through.
  onServer(`setsid sh -c "sleep 1; pkill -f 'sshd: .*@'" </dev/null >/dev/null 2>&1 &`);
  await page.waitForTimeout(8000);
  await shot("dropped", "After the server drops every SSH connection", [
    "A remote terminal whose connection dropped says so and offers Reattach and Close",
  ]);
  await page.getByRole("button", { name: "Reattach" }).first().click();
  await page.waitForTimeout(5000);
  await shot("reattached", "Reattached remote session", [
    "The shell is back with its earlier output: the tmux session survived on the server",
  ]);
  await page.getByRole("button", { name: "Refresh" }).click();
  await page.getByRole("tab", { name: "Changes" }).click();
  await shot("refreshed", "Refresh after the drop", ["The tab reloads over a new connection with no error"]);
});
