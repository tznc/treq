// Team projects: the Save project dialog's Owner picker, the project menu's
// Move to submenu, and the welcome page's Personal / team switcher with a
// teammate's project pulled by sync. Auth, organizations and
// organization_projects are the local Supabase stack
// (`npm run service-qa:up`); the dev build already points at it.
// Run with QA_TEAM_PROJECTS=1.
import { execFileSync, spawn, type ChildProcess } from "node:child_process";
import { mkdirSync, openSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { randomBytes, randomUUID } from "node:crypto";
import { chromium, expect, test, type Browser, type Page } from "@playwright/test";
import { appBinary, port } from "../env.ts";
import { appEnv, gitIn, makeFixture, writeFakeAgent } from "../harness.ts";
import { capture, theme } from "./capture.ts";
import { popupBox, settle, xPointer } from "./helpers.ts";

const appPort = port + 8;
const SUPABASE = "http://127.0.0.1:54321";
const DB = "postgresql://postgres:postgres@127.0.0.1:54322/postgres";
const ANON = "sb_publishable_ACJWlzQHlZjBrEguHvfOxg_3BJgxAaH";

type Session = { access_token: string; refresh_token: string; user: { id: string } };
type App = { app: ChildProcess; browser: Browser; page: Page; root: string };

const psql = (sql: string) =>
  execFileSync("psql", [DB, "-v", "ON_ERROR_STOP=1", "-At", "-c", sql]).toString().trim();

async function signUp(): Promise<Session> {
  const res = await fetch(`${SUPABASE}/auth/v1/signup`, {
    method: "POST",
    headers: { apikey: ANON, "Content-Type": "application/json" },
    body: JSON.stringify({
      email: `qa-team-projects-${randomBytes(4).toString("hex")}@example.com`,
      password: `Pw-${randomBytes(9).toString("base64url")}!a1`,
    }),
  });
  expect(res.status).toBe(200);
  return (await res.json()) as Session;
}

/** An organization "Acme" the user owns, with a teammate's project in it. */
function seedTeam(userId: string): string {
  const org = randomUUID();
  psql(`
    insert into public.organizations (id, name) values ('${org}', 'Acme');
    insert into public.organization_members (org_id, user_id, role) values ('${org}', '${userId}', 'owner');
    insert into public.organization_projects (org_id, name, repos, prompt)
      values ('${org}', 'Billing', '["acme/billing"]', 'Use pnpm');`);
  return org;
}

async function waitForServer(): Promise<void> {
  const deadline = Date.now() + 120_000;
  for (;;) {
    try {
      if ((await fetch(`http://127.0.0.1:${appPort}/`)).ok) return;
    } catch {}
    if (Date.now() > deadline) throw new Error("team projects QA app did not start");
    await new Promise((r) => setTimeout(r, 500));
  }
}

/** Launches signed in; with `openRepo` on a repo whose origin is acme/web, else on the welcome page. */
async function launch(session: Session, openRepo: boolean): Promise<App> {
  const fixture = makeFixture("treq-qa-team-");
  const { repo, home } = fixture;
  const git = (...args: string[]) => gitIn(repo, home, ...args);
  git("init", "-b", "main");
  writeFileSync(join(repo, "README.md"), "# web\n");
  git("add", ".");
  git("commit", "-m", "Initial commit");
  git("remote", "add", "origin", "git@github.com:acme/web.git");
  writeFakeAgent(fixture);
  const { DatabaseSync } = await import("node:sqlite");
  const dir = join(home, ".local/share/com.treq");
  mkdirSync(dir, { recursive: true });
  const db = new DatabaseSync(join(dir, "treq.db"));
  db.exec("CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT NOT NULL)");
  const set = db.prepare("INSERT OR REPLACE INTO settings (key, value) VALUES (?, ?)");
  set.run("theme", theme);
  set.run(
    "supabase_session",
    JSON.stringify({ accessToken: session.access_token, refreshToken: session.refresh_token }),
  );
  if (!openRepo) {
    // A personal project, so the Personal tab has something to list.
    db.exec(`CREATE TABLE IF NOT EXISTS projects (id TEXT PRIMARY KEY, name TEXT NOT NULL,
      created_at TEXT NOT NULL, last_opened_at TEXT NOT NULL, prompt TEXT NOT NULL DEFAULT '',
      context_paths TEXT NOT NULL DEFAULT '[]', org_id TEXT);
      CREATE TABLE IF NOT EXISTS project_repos (project_id TEXT NOT NULL, repo_path TEXT NOT NULL,
      position INTEGER NOT NULL, PRIMARY KEY (project_id, repo_path));`);
    const at = new Date().toISOString();
    db.prepare("INSERT INTO projects (id, name, created_at, last_opened_at) VALUES (?, ?, ?, ?)").run(
      "personal-1", "Side project", at, at,
    );
    db.prepare("INSERT INTO project_repos VALUES (?, ?, 0)").run("personal-1", repo);
  }
  db.close();
  const log = openSync(join(fixture.root, "app.log"), "w");
  const env: NodeJS.ProcessEnv = {
    ...appEnv(fixture),
    GPUI_PLAYWRIGHT: String(appPort),
    GDK_SCALE: process.env.GDK_SCALE ?? "2",
  };
  if (openRepo) env.TREQ_E2E_REPO = repo;
  const app = spawn(appBinary, [], { stdio: ["ignore", log, log], env });
  await waitForServer();
  const browser = await chromium.connect(`ws://127.0.0.1:${appPort}/`);
  const page = await (await browser.newContext()).newPage();
  await page.setViewportSize({ width: 1400, height: 1000 });
  return { app, browser, page, root: fixture.root };
}

async function stop({ app, browser }: App): Promise<void> {
  await browser.close().catch(() => {});
  const exited = new Promise((r) => app.once("exit", r));
  app.kill("SIGTERM");
  await Promise.race([exited, new Promise((r) => setTimeout(r, 10_000))]);
  if (app.exitCode === null && app.signalCode === null) app.kill("SIGKILL");
}

test.describe.configure({ mode: "serial" });

test("team projects: save to a team, then Move to", async () => {
  test.skip(process.env.QA_TEAM_PROJECTS !== "1", "needs QA_TEAM_PROJECTS=1 and the local Supabase stack");
  const session = await signUp();
  const org = seedTeam(session.user.id);
  const run = await launch(session, true);
  const { page } = run;
  try {
    const button = page.getByRole("button", { name: /^Project: / });
    await expect(button).toBeVisible({ timeout: 60_000 });
    // The account (and its organizations) loads after the window.
    await settle(page, 6000);
    // The menu is a popup window. Retry: a
    // click can land before the popup maps under Xvfb.
    const dialog = page.getByRole("button", { name: "Save project" });
    for (let i = 0; i < 4 && (await dialog.count()) === 0; i++) {
      await button.click();
      await settle(page, 1500);
      const menu = popupBox();
      // Rows: synced projects, a separator, Save Project…, New Window….
      if (menu) await xPointer(page, menu.x + 60, menu.y + menu.h - 41, 1);
      await settle(page, 1500);
    }
    await expect(dialog).toBeVisible();
    await capture(page, {
      name: "23-team-projects-01-save-owner",
      title: "Save project: Owner picker",
      expectations: [
        "Dialog 'Save project' with the name field and the repositories line",
        "An Owner row with a segmented Personal / Acme switcher, Personal selected",
      ],
    });
    await expect(page.getByRole("tab", { name: "Acme" })).toBeVisible({ timeout: 15_000 });
    await page.getByRole("tab", { name: "Acme" }).click();
    await page.keyboard.press("End");
    for (let i = 0; i < 40; i++) await page.keyboard.press("Backspace");
    await page.keyboard.type("Web app");
    await settle(page, 500);
    await capture(page, {
      name: "23-team-projects-02-save-owner-acme",
      title: "Save project: Acme picked",
      expectations: ["Name 'Web app', Owner switcher with Acme selected"],
    });
    await page.keyboard.press("Enter");
    await settle(page, 6000);
    await expect(page.getByRole("button", { name: "Project: Web app" })).toBeVisible();

    await page.getByRole("button", { name: "Project: Web app" }).click();
    await settle(page, 1000);
    await capture(page, {
      name: "23-team-projects-03-menu-move-to",
      title: "Project menu with Move to",
      screen: true,
      rest: false,
      expectations: ["The project menu lists '✓ Web app', Rename Project…, Project Instructions…, a Move to submenu, New Window…"],
    });
    const menu2 = popupBox();
    if (menu2) {
      // Move to is the row above New Window…, the last row.
      await xPointer(page, menu2.x + 60, menu2.y + menu2.h - 41, 0);
      await settle(page, 1000);
      await capture(page, {
        name: "23-team-projects-04-move-to-open",
        title: "Move to submenu",
        screen: true,
        rest: false,
        expectations: ["The Move to submenu lists Personal and '✓ Acme'"],
      });
    }
    void org;
  } finally {
    await stop(run);
  }
});

test("team projects: welcome page switcher with a teammate's synced project", async () => {
  test.skip(process.env.QA_TEAM_PROJECTS !== "1", "needs QA_TEAM_PROJECTS=1 and the local Supabase stack");
  const session = await signUp();
  seedTeam(session.user.id);
  const run = await launch(session, false);
  const { page } = run;
  try {
    const acme = page.getByRole("tab", { name: "Acme" });
    await expect(acme).toBeVisible({ timeout: 60_000 });
    await settle(page, 4000);
    await capture(page, {
      name: "23-team-projects-05-welcome-personal",
      title: "Welcome page: Personal projects",
      expectations: [
        "The Projects section has a Personal / Acme switcher, Personal selected",
        "It lists 'Side project' only",
      ],
    });
    await acme.click();
    await settle(page, 1000);
    await expect(page.getByText("Billing")).toBeVisible();
    await capture(page, {
      name: "23-team-projects-06-welcome-acme",
      title: "Welcome page: Acme projects",
      expectations: [
        "Acme selected; the list shows the teammate's 'Billing' project pulled by sync",
        "'Side project' is not listed",
      ],
    });
  } finally {
    await stop(run);
  }
});
