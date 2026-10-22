// A window opening on a folder that is not a repository, as a saved last
// repository whose `.git` was deleted does: an app on its own port with
// `TREQ_E2E_REPO` naming a plain folder. It shows onboarding and asks, in a
// native Yes/No dialog, whether to initialize a Git repository there:
// declined, the folder is left untouched; accepted, it opens as a repository.
import { execFileSync, spawn, type ChildProcess } from "node:child_process";
import { existsSync, mkdirSync, openSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { chromium, expect, test, type Browser, type Page } from "@playwright/test";
import { appBinary, port } from "../env.ts";
import { appEnv, makeFixture, writeFakeAgent } from "../harness.ts";
import { capture, theme } from "./capture.ts";

const plainPort = port + 2;

type App = { app: ChildProcess; browser: Browser; page: Page; folder: string };

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

/** Starts the app on a new plain folder. */
async function launch(): Promise<App> {
  const fixture = makeFixture("treq-qa-not-a-repo-", "plain-folder");
  const { repo, home } = fixture;
  writeFileSync(join(repo, "notes.txt"), "not a repository\n");
  writeFakeAgent(fixture);
  const { DatabaseSync } = await import("node:sqlite");
  const dir = join(home, ".local/share/com.treq");
  mkdirSync(dir, { recursive: true });
  const db = new DatabaseSync(join(dir, "treq.db"));
  db.exec("CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT NOT NULL)");
  db.prepare("INSERT OR REPLACE INTO settings (key, value) VALUES ('theme', ?)").run(theme);
  db.close();
  const log = openSync(join(fixture.root, "app.log"), "w");
  const app = spawn(appBinary, [], {
    stdio: ["ignore", log, log],
    env: {
      ...appEnv(fixture),
      GPUI_PLAYWRIGHT: String(plainPort),
      TREQ_E2E_REPO: repo,
      GDK_SCALE: process.env.GDK_SCALE ?? "2",
    },
  });
  await waitForServer();
  const browser = await chromium.connect(`ws://127.0.0.1:${plainPort}/`);
  const page = await (await browser.newContext()).newPage();
  await page.setViewportSize({ width: 1400, height: 900 });
  return { app, browser, page, folder: repo };
}

/** Stops the app and waits for its port to free, so the next one can bind it. */
async function stop({ app, browser }: App): Promise<void> {
  await browser.close().catch(() => {});
  const exited = new Promise((r) => app.once("exit", r));
  app.kill("SIGTERM");
  await Promise.race([exited, new Promise((r) => setTimeout(r, 10_000))]);
  if (app.exitCode === null && app.signalCode === null) app.kill("SIGKILL");
  for (let i = 0; i < 40; i++) {
    try {
      await fetch(`http://127.0.0.1:${plainPort}/`);
    } catch {
      return;
    }
    await new Promise((r) => setTimeout(r, 250));
  }
}

/** The native "Not a Git Repository" dialog's window, once it shows. */
async function question(): Promise<string> {
  const deadline = Date.now() + 15_000;
  for (;;) {
    try {
      const id = execFileSync("xdotool", ["search", "--onlyvisible", "--name", "Not a Git Repository"], {
        encoding: "utf8",
      }).split("\n").filter(Boolean)[0];
      if (id) return id;
    } catch {}
    if (Date.now() > deadline) throw new Error("no 'Not a Git Repository' dialog");
    await new Promise((r) => setTimeout(r, 300));
  }
}

/** Clicks No (the left half of the button row) or Yes (the right half). */
function answer(dialog: string, yes: boolean): void {
  const geo = Object.fromEntries(
    execFileSync("xdotool", ["getwindowgeometry", "--shell", dialog], { encoding: "utf8" })
      .split("\n").filter(Boolean).map((l) => l.split("=")),
  );
  const x = Number(geo.X) + Number(geo.WIDTH) * (yes ? 0.75 : 0.25);
  const y = Number(geo.Y) + Number(geo.HEIGHT) - 30;
  execFileSync("xdotool", ["mousemove", "--sync", String(Math.round(x)), String(Math.round(y)), "click", "1"]);
}

test.describe.configure({ mode: "serial" });

test("not-a-repository: asks, and declined leaves the folder alone", async () => {
  const run = await launch();
  try {
    const dialog = await question();
    await run.page.waitForTimeout(500);
    await capture(run.page, {
      name: "15-not-a-repository-01-asks",
      title: "Window opened on a folder that is not a repository",
      screen: true,
      expectations: [
        "Onboarding shows behind the dialog, not a repository sidebar",
        "A 'Not a Git Repository' Yes/No dialog asks to initialize the folder",
      ],
    });
    answer(dialog, false);
    const open = run.page.getByRole("button", { name: "Open Repository" });
    await expect(open).toBeVisible();
    await run.page.waitForTimeout(500);
    await capture(run.page, {
      name: "15-not-a-repository-02-declined",
      title: "Declined: onboarding with an error",
      screen: true,
      expectations: [
        "The onboarding card shows",
        "A 'Not a Git Repository' error toast",
      ],
    });
    expect(existsSync(join(run.folder, ".treq"))).toBe(false);
    expect(existsSync(join(run.folder, ".git"))).toBe(false);
  } finally {
    await stop(run);
  }
});

test("not-a-repository: initialized, it opens as a repository", async () => {
  const run = await launch();
  try {
    answer(await question(), true);
    await expect(run.page.getByRole("button", { name: "Open Repository" })).toBeHidden({
      timeout: 30_000,
    });
    await run.page.waitForTimeout(2000);
    await capture(run.page, {
      name: "15-not-a-repository-03-initialized",
      title: "Accepted: the folder opens as a new repository",
      screen: true,
      expectations: [
        "The repository view shows, named plain-folder in the sidebar",
        "A 'Repository Opened' toast",
      ],
    });
    expect(existsSync(join(run.folder, ".git"))).toBe(true);
  } finally {
    await stop(run);
  }
});
