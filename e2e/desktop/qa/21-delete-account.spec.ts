// Deleting a Treq account from Settings → Account, against the local
// Supabase stack (`npm run service-qa:up`) the dev build talks to. A real
// user signs up, creates an organization (so they are its last owner), and
// the app starts on its own port with that session in its database. The
// first delete is refused by the real delete-account Edge Function; once
// the organization is deleted, the second one deletes the account and the
// app signs out. Skipped when the local stack is not running.
import { spawn, type ChildProcess } from "node:child_process";
import { mkdirSync, openSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { randomBytes } from "node:crypto";
import { chromium, expect, test, type Browser, type Page } from "@playwright/test";
import { appBinary, port } from "../env.ts";
import { appEnv, gitIn, makeFixture, writeFakeAgent } from "../harness.ts";
import { capture, theme } from "./capture.ts";
import { settle } from "./helpers.ts";

const appPort = port + 6;
const SUPABASE = "http://127.0.0.1:54321";
// The local stack's publishable key, as package.json's dev env has it.
const ANON = "sb_publishable_ACJWlzQHlZjBrEguHvfOxg_3BJgxAaH";

type Session = { access_token: string; refresh_token: string };
type App = { app: ChildProcess; browser: Browser; page: Page };

async function stackUp(): Promise<boolean> {
  try {
    return (await fetch(`${SUPABASE}/auth/v1/health`, { headers: { apikey: ANON } })).ok;
  } catch {
    return false;
  }
}

async function auth(path: string, body: unknown): Promise<Response> {
  return fetch(`${SUPABASE}/auth/v1/${path}`, {
    method: "POST",
    headers: { apikey: ANON, "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
}

async function organizations(session: Session, body: unknown): Promise<Record<string, unknown>> {
  const res = await fetch(`${SUPABASE}/functions/v1/organizations`, {
    method: "POST",
    headers: {
      apikey: ANON,
      Authorization: `Bearer ${session.access_token}`,
      "Content-Type": "application/json",
    },
    body: JSON.stringify(body),
  });
  const json = (await res.json()) as Record<string, unknown>;
  expect(res.status, JSON.stringify(json)).toBe(200);
  return json;
}

async function waitForServer(): Promise<void> {
  const deadline = Date.now() + 120_000;
  for (;;) {
    try {
      if ((await fetch(`http://127.0.0.1:${appPort}/`)).ok) return;
    } catch {}
    if (Date.now() > deadline) throw new Error("delete-account app did not start");
    await new Promise((r) => setTimeout(r, 500));
  }
}

/** Starts the app on a new repository, signed in with `session`. */
async function launch(session: Session): Promise<App> {
  const fixture = makeFixture("treq-qa-delete-account-");
  const { repo, home } = fixture;
  const git = (...args: string[]) => gitIn(repo, home, ...args);
  git("init", "-b", "main");
  writeFileSync(join(repo, "README.md"), "# delete account\n");
  git("add", ".");
  git("commit", "-m", "Initial commit");
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
  db.close();
  const log = openSync(join(fixture.root, "app.log"), "w");
  const app = spawn(appBinary, [], {
    stdio: ["ignore", log, log],
    env: {
      ...appEnv(fixture),
      GPUI_PLAYWRIGHT: String(appPort),
      TREQ_E2E_REPO: repo,
      GDK_SCALE: process.env.GDK_SCALE ?? "2",
    },
  });
  await waitForServer();
  const browser = await chromium.connect(`ws://127.0.0.1:${appPort}/`);
  const page = await (await browser.newContext()).newPage();
  await page.setViewportSize({ width: 1400, height: 900 });
  return { app, browser, page };
}

async function stop({ app, browser }: App): Promise<void> {
  await browser.close().catch(() => {});
  const exited = new Promise((r) => app.once("exit", r));
  app.kill("SIGTERM");
  await Promise.race([exited, new Promise((r) => setTimeout(r, 10_000))]);
  if (app.exitCode === null && app.signalCode === null) app.kill("SIGKILL");
}

test("account: delete refused while last owner, then deleted and signed out", async () => {
  test.skip(!(await stackUp()), "needs the local Supabase stack (npm run service-qa:up)");

  const email = `qa-delete-${randomBytes(4).toString("hex")}@example.com`;
  const password = `Pw-${randomBytes(9).toString("base64url")}!a1`;
  const signup = await auth("signup", { email, password });
  expect(signup.status).toBe(200);
  const session = (await signup.json()) as Session;
  const created = await organizations(session, { action: "create", name: "Acme QA" });
  const orgId = (created.organization as { id: string }).id;

  const run = await launch(session);
  const { page } = run;
  try {
    await page.getByRole("button", { name: "Settings", exact: true }).click({ timeout: 30_000 });
    await page.getByRole("tab", { name: "Account" }).click();
    // GPUI text is not observable; the signed-in tab is, by its controls.
    await expect(page.getByRole("button", { name: "Sign Out" })).toBeVisible({ timeout: 30_000 });
    const open = page.getByTestId("account-delete");
    await open.scrollIntoViewIfNeeded();
    await page.mouse.move(1000, 860);
    await settle(page, 1200);
    await capture(page, {
      name: "21-delete-account-01-section",
      title: "Account tab, signed in, with the Delete account section",
      expectations: [
        "The signed-in user's email shows in the user card",
        "Below Sign Out, a red-bordered 'Delete account' section explains what deletion does",
        "A red 'Delete Account' button sits in that section",
      ],
    });

    await open.click();
    const dialog = page.getByRole("dialog", { name: "Delete your Treq account?" });
    await expect(dialog).toBeVisible();
    const confirm = page.getByTestId("account-delete-confirm");
    await expect(confirm).toBeDisabled();
    await settle(page, 800);
    await capture(page, {
      name: "21-delete-account-02-confirm-empty",
      title: "Delete account dialog before the email is typed",
      expectations: [
        "A dialog titled 'Delete your Treq account?' lists the consequences as bullet points",
        "An empty field asks to type the account email to confirm",
        "The red 'Delete Account' button is disabled",
      ],
    });

    await page.getByRole("textbox", { name: email }).fill(email);
    await expect(confirm).toBeEnabled();
    await confirm.click();
    await expect(
      page.getByRole("alert", {
        name: "You are the last owner of Acme QA. Make another member an owner, or delete the organization, before deleting your account.",
      }),
    ).toBeVisible({ timeout: 30_000 });
    await settle(page, 800);
    await capture(page, {
      name: "21-delete-account-03-last-owner",
      title: "The server refuses: the user is the last owner of an organization",
      expectations: [
        "The dialog stays open with the email still typed",
        "Red text: 'You are the last owner of Acme QA. Make another member an owner, or delete the organization, before deleting your account.'",
      ],
    });

    await organizations(session, { action: "delete_organization", organization_id: orgId });
    await confirm.click();
    await expect(page.getByRole("button", { name: "Sign in with Browser" })).toBeVisible({
      timeout: 30_000,
    });
    await expect(dialog).toHaveCount(0);
    const farewell = page.getByRole("alert", { name: "Sorry to see you go" });
    await expect(farewell).toBeVisible();
    await settle(page, 800);
    // The toast is a popup window of its own, so grab the whole screen.
    await capture(page, {
      name: "21-delete-account-04-signed-out",
      title: "Account deleted: toast, farewell, and the app signed out",
      screen: true,
      expectations: [
        "An 'Account deleted' success toast shows",
        "A 'Sorry to see you go' alert with a Dismiss button sits above 'Sign in to Treq'",
        "The delete dialog is gone and 'Sign in with Browser' shows",
      ],
    });
    await page.getByTestId("account-farewell-dismiss").click();
    await expect(farewell).toHaveCount(0);

    // The account is gone on the server: the password no longer signs in.
    const again = await auth("token?grant_type=password", { email, password });
    expect(again.status).toBe(400);
  } finally {
    await stop(run);
  }
});
