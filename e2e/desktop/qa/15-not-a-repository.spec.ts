// A window opening on a folder that is not a repository, as a saved last
// repository whose `.git` was deleted does: an app on its own port with
// `TREQ_E2E_REPO` naming a plain folder. It shows onboarding and asks, in an
// in-app confirmation, whether to initialize a Git repository there:
// cancelled, the folder is left untouched; initialized, it opens as a
// repository.
import { spawn, type ChildProcess } from "node:child_process";
import { existsSync, mkdirSync, openSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { chromium, expect, test, type Browser, type Locator, type Page } from "@playwright/test";
import { appBinary, port } from "../env.ts";
import { appEnv, makeFixture, serverUp, stopApp, writeFakeAgent } from "../harness.ts";
import { capture, theme } from "./capture.ts";
import { loaded, popupBox, screenChanged, screenPixels, xPointer } from "./helpers.ts";

// port + 2 is the fake Linear `launch.ts` starts.
const plainPort = port + 3;

type App = { app: ChildProcess; browser: Browser; page: Page; folder: string };

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
  await serverUp(app, plainPort);
  const browser = await chromium.connect(`ws://127.0.0.1:${plainPort}/`);
  const page = await (await browser.newContext()).newPage();
  await page.setViewportSize({ width: 1400, height: 900 });
  return { app, browser, page, folder: repo };
}

/** Stops the app and waits for its port to free, so the next one can bind it. */
async function stop({ app, browser }: App): Promise<void> {
  await browser.close();
  await stopApp(app);
  await expect(async () => {
    await expect(fetch(`http://127.0.0.1:${plainPort}/`)).rejects.toThrow();
  }, "the app's port frees").toPass({ intervals: [250], timeout: 10_000 });
}

/** The "Initialize a Git repository here?" confirmation's buttons, once it shows. */
async function question(page: Page): Promise<{ cancel: Locator; initialize: Locator }> {
  const cancel = page.getByTestId("confirm-cancel");
  const initialize = page.getByRole("button", { name: "Initialize", exact: true });
  await expect(initialize).toBeVisible({ timeout: 15_000 });
  await expect(page.getByRole("dialog", { name: "Initialize a Git repository here?" })).toBeVisible();
  return { cancel, initialize };
}

test.describe.configure({ mode: "serial" });

test("not-a-repository: asks, and cancelled leaves the folder alone", async () => {
  const run = await launch();
  try {
    const { cancel } = await question(run.page);
    await capture(run.page, {
      name: "15-not-a-repository-01-asks",
      title: "Window opened on a folder that is not a repository",
      screen: true,
      expectations: [
        "Onboarding shows behind the dialog, not a repository sidebar",
        "An in-app 'Initialize a Git repository here?' dialog names the folder, with Cancel and Initialize",
      ],
    });
    await cancel.click();
    const open = run.page.getByRole("button", { name: "Open repository…" });
    await expect(open).toBeVisible();
    await expect.poll(popupBox, { message: "the error toast shows" }).not.toBeNull();
    await capture(run.page, {
      name: "15-not-a-repository-02-declined",
      title: "Cancelled: onboarding with an error",
      screen: true,
      expectations: [
        "The onboarding card shows",
        "A 'Not a Git repository' error toast",
      ],
    });
    // The toast's Copy button sits left of its close button, `p-4` in.
    const toast = popupBox();
    if (!toast) throw new Error("the error toast closed");
    const copy = { x: toast.x + toast.w - 84, y: toast.y + 12, w: 32, h: 32 };
    const idle = screenPixels(copy);
    xPointer(toast.x + toast.w - 68, toast.y + 28, 1);
    const copied = await screenChanged(copy, idle);
    await capture(run.page, {
      name: "15-not-a-repository-02b-copied",
      title: "Error toast copied",
      screen: true,
      expectations: ["The error toast's Copy icon is a green check"],
    });
    // The check shows for a while, then swaps back to the Copy icon.
    await screenChanged(copy, copied);
    await capture(run.page, {
      name: "15-not-a-repository-02c-copy-reverted",
      title: "Error toast copy reverted",
      screen: true,
      expectations: ["The error toast shows the Copy icon again, not the check"],
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
    await (await question(run.page)).initialize.click();
    await expect(run.page.getByRole("button", { name: "Open repository…" })).toBeHidden({
      timeout: 30_000,
    });
    await expect(run.page.getByText("plain-folder").first()).toBeVisible();
    await loaded(run.page);
    await expect.poll(popupBox, { message: "the Opened toast shows" }).not.toBeNull();
    await capture(run.page, {
      name: "15-not-a-repository-03-initialized",
      title: "Accepted: the folder opens as a new repository",
      screen: true,
      expectations: [
        "The repository view shows, named plain-folder in the sidebar",
        "An 'Opened plain-folder' toast",
      ],
    });
    expect(existsSync(join(run.folder, ".git"))).toBe(true);
  } finally {
    await stop(run);
  }
});
