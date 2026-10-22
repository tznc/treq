// A window opening on a folder that is not a repository, as a saved last
// repository whose `.git` was deleted does: a second app on its own port
// with `TREQ_E2E_REPO` naming a plain folder. It should show onboarding and
// an error, and leave the folder untouched.
import { spawn, type ChildProcess } from "node:child_process";
import { existsSync, mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { chromium, expect, test, type Browser, type Page } from "@playwright/test";
import { appBinary, port } from "../env.ts";
import { appEnv, makeFixture, writeFakeAgent } from "../harness.ts";
import { capture, theme } from "./capture.ts";

const plainPort = port + 2;
let app: ChildProcess;
let browser: Browser;
let page: Page;
const fixture = makeFixture("treq-qa-not-a-repo-", "plain-folder");

async function waitForServer(): Promise<void> {
  const deadline = Date.now() + 120_000;
  for (;;) {
    try {
      const res = await fetch(`http://127.0.0.1:${plainPort}/`);
      if (res.ok) return;
    } catch {}
    if (Date.now() > deadline) throw new Error("not-a-repository app did not start");
    await new Promise((r) => setTimeout(r, 500));
  }
}

test.describe.configure({ mode: "serial" });

test.beforeAll(async () => {
  const { repo, home } = fixture;
  writeFileSync(join(repo, "notes.txt"), "not a repository\n");
  writeFakeAgent(fixture);
  {
    const { DatabaseSync } = await import("node:sqlite");
    const dir = join(home, ".local/share/com.treq");
    mkdirSync(dir, { recursive: true });
    const db = new DatabaseSync(join(dir, "treq.db"));
    db.exec("CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT NOT NULL)");
    db.prepare("INSERT OR REPLACE INTO settings (key, value) VALUES ('theme', ?)").run(theme);
    db.close();
  }
  app = spawn(appBinary, [], {
    stdio: "ignore",
    env: {
      ...appEnv(fixture),
      GPUI_PLAYWRIGHT: String(plainPort),
      TREQ_E2E_REPO: repo,
      GDK_SCALE: process.env.GDK_SCALE ?? "2",
    },
  });
  await waitForServer();
  browser = await chromium.connect(`ws://127.0.0.1:${plainPort}/`);
  page = await (await browser.newContext()).newPage();
  await page.setViewportSize({ width: 1400, height: 900 });
});

test.afterAll(async () => {
  await browser?.close().catch(() => {});
  app?.kill("SIGTERM");
});

test("not-a-repository: opens onboarding with an error", async () => {
  // Captured before asserting, so a broken build still shows what it does;
  // soon after onboarding shows, while the toast is still up.
  const open = page.getByRole("button", { name: "Open Repository" });
  await open.waitFor({ timeout: 15_000 }).catch(() => {});
  await page.waitForTimeout(500);
  await capture(page, {
    name: "15-not-a-repository-opened",
    title: "Window opened on a folder that is not a repository",
    screen: true,
    expectations: [
      "The onboarding card shows, not a repository sidebar",
      "A 'Not a Git Repository' error toast names the folder",
    ],
  });
  await expect(open).toBeVisible();
  expect(existsSync(join(fixture.repo, ".treq"))).toBe(false);
});
