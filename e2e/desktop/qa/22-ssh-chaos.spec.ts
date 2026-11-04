// Chaos on the remote (SSH) workspace flows, against the Docker host that
// `ssh-server/up.sh` starts (same QA_SSH_* variables as 18-ssh-remote).
// Targeted faults: wrong host, port and user on Add SSH host; a
// double-submitted New workspace; the host paused (`docker pause`) and
// stopped (`docker stop`) mid-operation, then back; the remote tmux killed;
// a workspace directory deleted and a commit made on the server behind the
// app; switching between a local and the remote repository; saving a
// project that holds the remote repository. Then a seeded monkey
// (QA_SEED, CHAOS_STEPS) over the remote screens, with a host fault mixed
// in now and then. After every step: the process is alive, Playwright
// answers and the log has no `panicked`; after the host returns, the
// screen leaves its spinner (an error or the data, not "Loading" forever).
import { execFileSync, spawn, type ChildProcess } from "node:child_process";
import { copyFileSync, mkdirSync, openSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { chromium, expect, test, type Browser, type Page } from "@playwright/test";
import { appBinary, port } from "../env.ts";
import { appEnv, makeFixture } from "../harness.ts";
import { capture, theme } from "./capture.ts";
import { popupBox, xPointer } from "./helpers.ts";

const env = {
  host: process.env.QA_SSH_HOST ?? "",
  port: process.env.QA_SSH_PORT ?? "22",
  user: process.env.QA_SSH_USER ?? "",
  key: process.env.QA_SSH_KEY ?? "",
  repo: process.env.QA_SSH_REPO ?? "",
};
const CONTAINER = process.env.QA_SSH_CONTAINER ?? "treq-qa-ssh";
const STEPS = Number(process.env.CHAOS_STEPS ?? 40);
let seed = Number(process.env.QA_SEED ?? 22);
const seed0 = seed;
const appPort = port + 8;
const MACHINE = "QA chaos host";
const repoName = env.repo.split("/").pop() ?? "";

let app: ChildProcess;
let browser: Browser;
let page: Page;
let fixture: ReturnType<typeof makeFixture>;
let logPath = "";
/** Where the running app's output starts in the log. */
let logStart = 0;
let shotNo = 0;
const trail: string[] = [];
const findings: string[] = [];

function rand(): number {
  // mulberry32
  seed |= 0;
  seed = (seed + 0x6d2b79f5) | 0;
  let t = Math.imul(seed ^ (seed >>> 15), 1 | seed);
  t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
  return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
}
const pick = <T>(xs: readonly T[]): T => xs[Math.floor(rand() * xs.length)]!;

function note(what: string) {
  trail.push(what);
  console.log(`[ssh-chaos seed=${seed0}] ${what}`);
}

function docker(...args: string[]): void {
  try {
    execFileSync("docker", args, { stdio: "pipe" });
  } catch (e) {
    note(`docker ${args.join(" ")} failed: ${e}`);
  }
}

/** Runs `script` on the server, outside the app. */
function onServer(script: string): string {
  return execFileSync(
    "ssh",
    [
      "-i", env.key, "-p", env.port,
      "-o", "StrictHostKeyChecking=no", "-o", "UserKnownHostsFile=/dev/null", "-o", "LogLevel=ERROR",
      "-o", "ConnectTimeout=10",
      `${env.user}@${env.host}`, script,
    ],
    { encoding: "utf8", timeout: 60_000 },
  );
}

/** Waits until sshd answers again after a restart. */
async function hostBack(): Promise<void> {
  const deadline = Date.now() + 60_000;
  for (;;) {
    try {
      onServer("true");
      return;
    } catch {
      if (Date.now() > deadline) throw new Error("ssh host did not come back");
      await new Promise((r) => setTimeout(r, 1000));
    }
  }
}

/** `launchOnce`, again when wgpu panics on the first frame under Xvfb. */
async function launch(): Promise<void> {
  for (let attempt = 1; ; attempt++) {
    try {
      return await launchOnce();
    } catch (error) {
      app?.kill("SIGKILL");
      if (attempt >= 3) throw error;
      note(`app start failed (${error}); starting again`);
      await new Promise((r) => setTimeout(r, 2000));
    }
  }
}

async function launchOnce(): Promise<void> {
  logStart = appLog().length;
  const log = openSync(logPath, "a");
  app = spawn(appBinary, [], {
    stdio: ["ignore", log, log],
    env: {
      ...appEnv(fixture),
      GPUI_PLAYWRIGHT: String(appPort),
      GDK_SCALE: process.env.GDK_SCALE ?? "2",
      RUST_LOG: process.env.RUST_LOG ?? "info",
    },
  });
  const deadline = Date.now() + 120_000;
  for (;;) {
    try {
      if ((await fetch(`http://127.0.0.1:${appPort}/`)).ok) break;
    } catch {}
    if (app.exitCode !== null || app.signalCode) throw new Error("app exited on start");
    if (Date.now() > deadline) throw new Error("ssh chaos app did not start");
    await new Promise((r) => setTimeout(r, 500));
  }
  for (let attempt = 1; ; attempt++) {
    try {
      browser = await chromium.connect(`ws://127.0.0.1:${appPort}/`);
      page = await (await browser.newContext()).newPage();
      break;
    } catch (error) {
      await browser?.close().catch(() => {});
      if (attempt >= 10) throw error;
      await new Promise((r) => setTimeout(r, 1000));
    }
  }
  await page.setViewportSize({ width: 1400, height: 900 });
}

function appLog(): string {
  try {
    return readFileSync(logPath, "utf8");
  } catch {
    return "";
  }
}

/** The app is alive, answers Playwright and has not panicked. */
async function alive(what: string): Promise<void> {
  const ctx = `after: ${what}\nseed ${seed0} trail: ${trail.slice(-25).join(" | ")}`;
  expect(app.exitCode, `app exited ${ctx}`).toBeNull();
  expect(app.signalCode, `app killed ${ctx}`).toBeNull();
  const answered = await Promise.race([
    page.ariaSnapshot().then(() => true, () => false),
    new Promise<boolean>((r) => setTimeout(() => r(false), 15_000)),
  ]);
  expect(answered, `Playwright got no answer ${ctx}`).toBe(true);
  const log = appLog().slice(logStart);
  const at = log.indexOf("panicked");
  expect(at, `panic ${ctx}:\n${log.slice(Math.max(0, at - 400), at + 1500)}`).toBe(-1);
}

/** After the host returns, nothing still reads "Loading"/"Connecting…". */
async function recovered(what: string, ms = 60_000): Promise<void> {
  const busy = /Loading|Connecting…|Reconnecting…|Creating…|Opening…/;
  const deadline = Date.now() + ms;
  let snap = "";
  while (Date.now() < deadline) {
    snap = await page.ariaSnapshot().catch(() => "");
    if (!busy.test(snap)) return;
    await page.waitForTimeout(2000);
  }
  const line = snap.split("\n").find((l) => busy.test(l));
  findings.push(`${what}: still busy after ${ms / 1000}s (${line?.trim()})`);
  note(`STUCK ${what}: ${line?.trim()}`);
}

async function shot(what: string, title: string, expectations: string[]): Promise<void> {
  shotNo += 1;
  const name = `22-ssh-chaos-s${seed0}-${String(shotNo).padStart(2, "0")}-${what}`;
  await page.waitForTimeout(800);
  await capture(page, { name, title, screen: true, rest: false, expectations });
  note(`shot ${name}-${theme}.png`);
}

const workspaceRow = (name: string) => page.getByRole("button", { name, exact: true });

/** Opens Add SSH host from the Overview's Other machines + and fills it. */
async function addMachine(name: string, host: string, sshPort: string, user: string): Promise<void> {
  // Once a host is saved, SSH HOSTS has its own + ("Add SSH host").
  const other = page.getByRole("button", { name: "Add another machine" });
  if (await other.isVisible().catch(() => false)) await other.click();
  await page.getByRole("button", { name: "Add SSH host" }).first().click();
  await page.waitForTimeout(1200);
  const fields = page.getByRole("textbox");
  await fields.nth(0).fill(name);
  await fields.nth(1).fill(host);
  await fields.nth(2).fill(sshPort);
  await fields.nth(3).fill(user);
  await page.getByRole("button", { name: "Continue" }).click();
}

test.describe.configure({ mode: "serial" });
test.skip(!env.host, "QA_SSH_HOST is not set");

test.beforeAll(async () => {
  onServer(
    `set -e; rm -rf '${env.repo}'; mkdir -p '${env.repo}'; cd '${env.repo}'
     git init -q -b main
     git config user.name QA; git config user.email qa@example.com
     printf '# demo\\n' > README.md; git add .; git commit -qm 'Initial commit'
     printf 'x\\n' > lib.txt; git add .; git commit -qm 'Add lib'
     treq pty-remote reset --repo / --format json >/dev/null 2>&1 || true
     tmux kill-server 2>/dev/null || true`,
  );
  fixture = makeFixture("treq-qa-sshchaos-");
  logPath = join(fixture.root, "app.log");
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
  note(`seed ${seed0}, log ${logPath}`);
  await launch();
});

test.afterAll(async () => {
  docker("unpause", CONTAINER);
  docker("start", CONTAINER);
  await browser?.close().catch(() => {});
  app?.kill("SIGTERM");
  console.log(`[ssh-chaos seed=${seed0}] findings:\n${findings.join("\n") || "(none)"}`);
  console.log(`[ssh-chaos seed=${seed0}] trail:\n${trail.join("\n")}`);
});

test("ssh chaos: wrong host, port and user on Add SSH host", async () => {
  test.setTimeout(600_000);
  await expect(page.getByRole("button", { name: "Add another machine" })).toBeVisible({ timeout: 60_000 });
  const bad: [string, string, string, string][] = [
    ["bad-host", "no-such-host.invalid", env.port, env.user],
    ["bad-port", env.host, "1", env.user],
    ["bad-user", env.host, env.port, "nobody-qa"],
  ];
  for (const [what, host, p, user] of bad) {
    note(`add machine ${what}: ${user}@${host}:${p}`);
    await addMachine(`QA ${what}`, host, p, user);
    // A wrong user is still asked to trust the (real) host key first.
    const trust = page.getByRole("button", { name: "Trust and connect" });
    if (await trust.isVisible({ timeout: 15_000 }).catch(() => false)) await trust.click();
    await page.waitForTimeout(5000);
    await alive(`add machine ${what}`);
    await recovered(`add machine ${what}`, 45_000);
    await shot(`add-${what}`, `Add SSH host with a wrong ${what.slice(4)}`, [
      "The form or a toast says why the connection failed; no endless spinner",
      "Continue is usable again (the form can be corrected)",
    ]);
    await page.keyboard.press("Escape");
    await page.waitForTimeout(800);
    await page.keyboard.press("Escape");
    await page.waitForTimeout(800);
  }
  note("add machine: the real one");
  await addMachine(MACHINE, env.host, env.port, env.user);
  await page.getByRole("button", { name: "Trust and connect" }).click({ timeout: 30_000 });
  const folder = page.getByRole("listitem", { name: repoName, exact: true });
  await expect(folder).toBeVisible({ timeout: 30_000 });
  await alive("connected");
  await shot("machines-after-bad", "Machines after three failed and one good add", [
    `'${MACHINE}' is listed under SSH HOSTS; the failed ones are either absent or marked unreachable`,
  ]);
  await folder.click();
  await page.getByRole("button", { name: "Open", exact: true }).click({ timeout: 30_000 });
  await expect(page.getByRole("tab", { name: "Commits" })).toBeVisible({ timeout: 60_000 });
  await alive("opened remote repo");
});

test("ssh chaos: double-submit New workspace", async () => {
  await page.getByRole("button", { name: "New workspace…" }).first().click();
  await page.getByLabel("Workspaces", { exact: true }).fill("Chaos one");
  const create = page.getByRole("button", { name: "Create workspace" });
  await expect(create).toBeEnabled({ timeout: 20_000 });
  note("double-click Create workspace");
  await create.dblclick();
  await create.click({ timeout: 1000 }).catch(() => {});
  await page.keyboard.press("Enter");
  await expect(workspaceRow("Chaos one")).toBeVisible({ timeout: 60_000 });
  await page.waitForTimeout(4000);
  await alive("double-submit");
  const dirs = onServer(`ls '${env.repo}/.treq/workspaces'`).trim().split(/\s+/);
  note(`server workspaces: ${dirs.join(",")}`);
  if (dirs.length !== 1) findings.push(`double-submit made ${dirs.length} workspaces: ${dirs.join(",")}`);
  await shot("double-submit", "After a double-submitted New workspace", [
    "Exactly one 'Chaos one' row in the sidebar; no error toast",
  ]);
});

test("ssh chaos: host paused and stopped mid-operation", async () => {
  test.setTimeout(600_000);
  // Paused while Commits reloads: the read hangs, then the host returns.
  await page.getByRole("tab", { name: "Commits" }).click();
  note("docker pause during Refresh");
  docker("pause", CONTAINER);
  await page.getByRole("button", { name: "Refresh" }).first().click().catch(() => {});
  await page.waitForTimeout(8000);
  await alive("refresh while paused");
  await shot("paused", "Refresh while the host is paused", [
    "A loading, Reconnecting… or offline state; the window still responds",
  ]);
  docker("unpause", CONTAINER);
  await recovered("refresh after unpause", 90_000);
  await alive("unpaused");
  await shot("unpaused", "After the host is unpaused", ["Commits lists 'Add lib' again, or an error with Retry/Refresh"]);

  // Stopped while creating a workspace.
  await page.getByRole("button", { name: "New workspace…" }).first().click();
  await page.getByLabel("Workspaces", { exact: true }).fill("Chaos stopped");
  await expect(page.getByRole("button", { name: "Create workspace" })).toBeEnabled({ timeout: 20_000 });
  await page.getByRole("button", { name: "Create workspace" }).click();
  note("docker stop during Create workspace");
  docker("stop", "-t", "0", CONTAINER);
  await page.waitForTimeout(10_000);
  await alive("create while stopped");
  await shot("stopped-create", "Create workspace when the host stops", [
    "An error says the host is unreachable (toast, dialog error or Offline banner); no endless Creating…",
  ]);
  docker("start", CONTAINER);
  await hostBack();
  await page.keyboard.press("Escape");
  await page.getByRole("button", { name: "Refresh" }).first().click().catch(() => {});
  await recovered("after docker start", 90_000);
  await alive("host started");
  await shot("restarted", "After the host restarts", [
    "The remote repository loads again over a new connection; Chaos one is listed",
  ]);
});

test("ssh chaos: changes behind the app (tmux, deleted dir, new commit)", async () => {
  test.setTimeout(400_000);
  // A remote shell, then its tmux (if the host uses it) and daemon killed.
  const row = workspaceRow("Chaos one");
  await row.click();
  await row.hover();
  await row.getByRole("button", { name: "Open shell" }).click().catch((e) => note(`open shell: ${e}`));
  await page.waitForTimeout(4000);
  note("kill remote tmux and treq daemon");
  onServer("tmux kill-server 2>/dev/null; pkill -f '[t]req pty' 2>/dev/null; true");
  await page.waitForTimeout(8000);
  await alive("killed terminals");
  await shot("terminals-killed", "Remote terminal holder killed", [
    "The pane shows it ended (Reattach/Close or an exit line), not a frozen terminal",
  ]);

  note("new commit on the remote main");
  onServer(`cd '${env.repo}' && printf 'y\\n' > behind.txt && git add behind.txt && git -c user.name=QA -c user.email=qa@example.com commit -qm 'Commit behind the app'`);
  // main's home row, whose Commits list main.
  await page.getByRole("button", { name: "main", exact: true }).first().click().catch((e) => note(`main row: ${e}`));
  await page.getByRole("tab", { name: "Commits" }).click();
  await page.getByRole("button", { name: "Refresh" }).first().click().catch(() => {});
  await recovered("refresh after remote commit");
  await page.waitForTimeout(3000);
  await alive("remote commit");
  await shot("remote-commit", "Commits after a commit on the server", [
    "'Commit behind the app' shows (on main / the home row's Commits)",
  ]);

  note("delete the workspace dir behind the app");
  note(onServer(`rm -rf '${env.repo}/.treq/workspaces/'*chaos-one*; ls '${env.repo}/.treq/workspaces'`));
  await row.click().catch(() => {});
  await page.getByRole("tab", { name: "Changes" }).click().catch(() => {});
  await page.getByRole("button", { name: "Refresh" }).first().click().catch(() => {});
  await recovered("deleted workspace dir");
  await page.waitForTimeout(2000);
  await alive("deleted workspace dir");
  await shot("deleted-dir", "A remote workspace whose folder was deleted", [
    "An error says the workspace folder is missing (or the row goes away); no endless loading",
  ]);
  // Archive what's left.
  await row.hover().catch(() => {});
  const archive = page.getByRole("button", { name: /^Archive( workspace)?$/ }).first();
  if (await archive.isVisible().catch(() => false)) {
    note("archive deleted workspace");
    await archive.click();
    await page.getByRole("button", { name: /^Archive/ }).last().click({ timeout: 3000 }).catch(() => {});
    await page.waitForTimeout(4000);
    await alive("archive deleted workspace");
    await shot("archived", "After archiving the deleted workspace", ["The row is gone or an error explains why not"]);
  } else note("no Archive control visible");
});

test("ssh chaos: switch local and remote, save project", async () => {
  test.setTimeout(300_000);
  // Local repository recent: open it via the command palette's Overview if
  // there is one; otherwise relaunch on it, which leaves the remote recent.
  await browser.close().catch(() => {});
  app.kill("SIGTERM");
  await new Promise((r) => app.once("exit", r));
  const { DatabaseSync } = await import("node:sqlite");
  const db = new DatabaseSync(join(fixture.home, ".local/share/com.treq/treq.db"));
  db.exec(
    "DELETE FROM settings WHERE key IN ('last_opened_remote_repo_id', 'last_opened_remote_repo', 'last_opened_repo_path')",
  );
  db.close();
  await launch();
  await expect(page.getByRole("button", { name: "Add SSH host" }).first()).toBeVisible({ timeout: 60_000 });
  const remoteRecent = page.getByRole("listitem", { name: env.repo }).first();
  for (let i = 0; i < 4; i++) {
    note(`switch ${i}: open remote recent`);
    await remoteRecent.click({ timeout: 20_000 });
    await page.waitForTimeout(i % 2 ? 300 : 3000);
    await alive(`switch ${i}`);
    // Back to the Overview without waiting for the reads.
    await page.keyboard.press("Control+k");
    await page.keyboard.type("Overview");
    await page.waitForTimeout(400);
    await page.keyboard.press("Enter");
    await page.waitForTimeout(800);
    await page.keyboard.press("Escape");
    if (!(await remoteRecent.isVisible().catch(() => false))) {
      note("no way back to the Overview from the palette; stopping the switch loop");
      break;
    }
  }
  await recovered("after switching", 60_000);
  await shot("switched", "After fast switching", ["The remote repository shows its data, no stuck loading"]);

  const button = page.getByRole("button", { name: /^Project: / });
  if ((await button.count()) === 0) {
    note("no project button on the remote window");
    findings.push("remote window shows no Project button: a remote repo can't be saved in a project");
    return;
  }
  await button.click();
  await page.waitForTimeout(1000);
  // The menu is a popup: "Save Project…" is its first row, clicked through X.
  const menu = popupBox();
  if (!menu) throw new Error("no project menu");
  await xPointer(page, menu.x + 60, menu.y + 18, 1);
  await page.waitForTimeout(1200);
  await shot("save-project", "Save project with the remote repository", [
    "The Save project dialog lists the remote repository",
  ]);
  await page.keyboard.press("End");
  for (let i = 0; i < 40; i++) await page.keyboard.press("Backspace");
  await page.keyboard.type("Remote QA");
  await page.keyboard.press("Enter");
  await page.waitForTimeout(6000);
  await alive("save project");
  if ((await page.getByRole("button", { name: "Project: Remote QA" }).count()) === 0)
    findings.push("saving a project with the remote repository did not rename the Project button");
  await shot("project-saved", "After saving the project", ["The Project button names the saved project; no error"]);
});

test("ssh chaos: seeded monkey over the remote screens", async () => {
  test.setTimeout(STEPS * 30_000 + 120_000);
  const keys = ["Escape", "Tab", "Enter", "Control+k", "Control+p", "Control+j", "Control+]", "Control+\\", "?", "Control+=", "Control+-", "Control+0"];
  const texts = ["", "a", "ünïcødé ☃", "x".repeat(300), "../../etc", "'; rm -rf /"];
  for (let step = 0; step < STEPS; step++) {
    const r = rand();
    if (r < 0.12) {
      const fault = pick(["pause", "stop", "tmux", "drop"] as const);
      note(`#${step} fault ${fault}`);
      if (fault === "pause") {
        docker("pause", CONTAINER);
        await page.waitForTimeout(4000 + rand() * 4000);
        docker("unpause", CONTAINER);
      } else if (fault === "stop") {
        docker("stop", "-t", "0", CONTAINER);
        await page.waitForTimeout(3000);
        docker("start", CONTAINER);
        await hostBack();
      } else if (fault === "tmux") {
        onServer("tmux kill-server 2>/dev/null; true");
      } else {
        onServer(`setsid sh -c "sleep 1; pkill -f 'sshd: .*@'" </dev/null >/dev/null 2>&1 &`);
      }
      await recovered(`monkey #${step} ${fault}`, 60_000);
    } else if (r < 0.55) {
      const role = pick(["button", "tab", "listitem", "checkbox"] as const);
      const all = await page.getByRole(role).all().catch(() => []);
      const visible = [];
      for (const el of all.slice(0, 80)) if (await el.isVisible().catch(() => false)) visible.push(el);
      if (!visible.length) continue;
      const el = pick(visible);
      const name = (await el.getAttribute("aria-label").catch(() => null)) ?? (await el.textContent().catch(() => "")) ?? "";
      // Never quit, remove the host, reset terminals or sign out from the monkey.
      if (/Quit|Remove host|Reset terminals|Delete|Sign (in|out)|Use tmux|Use Treq daemon|Install/i.test(name)) continue;
      note(`#${step} click ${role} '${name.slice(0, 40)}'`);
      await el.click({ timeout: 3000 }).catch(() => {});
    } else if (r < 0.85) {
      const key = pick(keys);
      note(`#${step} key ${key}`);
      await page.keyboard.press(key).catch(() => {});
    } else {
      const t = pick(texts);
      note(`#${step} type ${JSON.stringify(t.slice(0, 20))}`);
      await page.keyboard.type(t).catch(() => {});
    }
    await page.waitForTimeout(300 + rand() * 900);
    await alive(`monkey #${step}`);
    if (step % 10 === 9) await shot(`monkey-${step + 1}`, `Monkey after ${step + 1} steps`, ["The window renders; no blank or frozen screen"]);
  }
  docker("unpause", CONTAINER);
  docker("start", CONTAINER);
  await hostBack();
  await page.keyboard.press("Escape");
  await recovered("monkey end", 90_000);
  await alive("monkey end");
  await shot("monkey-end", "Monkey done, host up", ["The window recovered: data or an error, no spinner"]);
});
