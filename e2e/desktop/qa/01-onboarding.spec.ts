// Onboarding: a second app with a fresh home and app data dir and no
// repository, on its own port, so the first-run screen shows. It opens the
// demo-free fixture repository through the native folder picker (GTK),
// which `xdotool` types a path into.
import { execFileSync, spawn, type ChildProcess } from "node:child_process";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { chromium, expect, test, type Browser, type Page } from "@playwright/test";
import { appBinary, port } from "../env.ts";
import { appEnv, gitIn, makeFixture, writeFakeAgent } from "../harness.ts";
import { capture, screenShot, outDir, theme } from "./capture.ts";

const onboardingPort = port + 1;
let app: ChildProcess;
let browser: Browser;
let page: Page;
const fixture = makeFixture("treq-qa-onboarding-", "hello-repo");

async function waitForServer(): Promise<void> {
  const deadline = Date.now() + 120_000;
  for (;;) {
    try {
      const res = await fetch(`http://127.0.0.1:${onboardingPort}/`);
      if (res.ok) return;
    } catch {}
    if (Date.now() > deadline) throw new Error("onboarding app did not start");
    await new Promise((r) => setTimeout(r, 500));
  }
}

test.describe.configure({ mode: "serial" });

test.beforeAll(async () => {
  const { repo, home } = fixture;
  gitIn(repo, home, "init", "-b", "main");
  writeFileSync(join(repo, "README.md"), "# hello-repo\n");
  gitIn(repo, home, "add", ".");
  gitIn(repo, home, "commit", "-m", "Initial commit");
  writeFakeAgent(fixture);
  {
    // A fresh install says "system", which is light under Xvfb; pin the
    // run's theme so both runs show their own.
    const { DatabaseSync } = await import("node:sqlite");
    const { mkdirSync } = await import("node:fs");
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
      GPUI_PLAYWRIGHT: String(onboardingPort),
      GDK_SCALE: process.env.GDK_SCALE ?? "2",
    },
  });
  await waitForServer();
  browser = await chromium.connect(`ws://127.0.0.1:${onboardingPort}/`);
  page = await (await browser.newContext()).newPage();
  await page.setViewportSize({ width: 1400, height: 900 });
});

test.afterAll(async () => {
  await browser?.close().catch(() => {});
  app?.kill("SIGTERM");
});

test("onboarding: first run and open repository", async () => {
  const open = page.getByRole("button", { name: "Open repository…" });
  await expect(open).toBeVisible({ timeout: 60_000 });
  await page.waitForTimeout(1500);
  await capture(page, {
    name: "01-onboarding-first-run",
    title: "Onboarding: first run, no repository",
    expectations: [
      "'Open a repository' heading and helper text are centred and unclipped",
      "Open repository… is the primary action, aligned with the text",
      "Background and text colours match the theme",
    ],
  });

  // The native folder picker: GTK draws it, so the X screen is grabbed.
  await open.click();
  await page.waitForTimeout(2500);
  screenShot(join(outDir, `01-onboarding-folder-picker-${theme}.png`));
  writeFileSync(
    join(outDir, `01-onboarding-folder-picker-${theme}.json`),
    JSON.stringify({
      file: `01-onboarding-folder-picker-${theme}.png`,
      title: "Onboarding: native Select Folder dialog",
      what_to_check: [
        "The GTK 'Select folder' dialog opens over the app window",
        "The app window behind it is not blank or frozen",
      ],
    }, null, 2),
  );
  // Under Xvfb with no window manager GTK's chooser draws and takes typing
  // but never lists a folder or enables Open, so the run cancels it; the
  // main app (TREQ_E2E_REPO) covers an opened repository.
  closePicker();
  await page.waitForTimeout(1000);
  await capture(page, {
    name: "01-onboarding-picker-cancelled",
    title: "Onboarding after cancelling the folder picker",
    expectations: [
      "The onboarding card is back, unchanged, with no error toast",
      "The window repaints fully after the dialog closes",
    ],
  });

  // Signed out, Treq Cloud is a sign-in button, not a machine group.
  await expect(page.getByRole("button", { name: "Sign in to Treq Cloud" })).toBeVisible();
  // Other machines' + offers Add SSH host, which opens the own-VM form.
  await page.getByRole("button", { name: "Add another machine" }).click();
  await page.getByRole("button", { name: "Add SSH host" }).click();
  await page.waitForTimeout(1500);
  await capture(page, {
    name: "01-onboarding-ssh-own-vm",
    title: "Open a repository: Add SSH host (your own VM) form",
    screen: true,
    expectations: [
      "The own-VM form opens directly; Cancel and Continue at the bottom and no X, as it has an action pair",
      "Field labels and placeholders readable in this theme",
      "No network is needed to show the form; no stuck spinner",
    ],
  });
  // The dialog closes with Cancel or Escape, as every other dialog does.
  const close = page.getByRole("button", { name: "Cancel" });
  await expect(close).toBeVisible();
  await page.keyboard.press("Escape");
  await page.waitForTimeout(1000);
  await expect(close).toHaveCount(0);
  await capture(page, {
    name: "01-onboarding-ssh-after-escape",
    title: "Remote dialog after pressing Escape",
    screen: true,
    expectations: ["The remote dialog is closed and the onboarding card shows"],
  });
  await page.getByRole("button", { name: "Add another machine" }).click();
  await page.getByRole("button", { name: "Add SSH host" }).click();
  await page.waitForTimeout(1000);
  await close.click();
  await page.waitForTimeout(1000);
  await expect(close).toHaveCount(0);
  await page.getByRole("button", { name: "Add another machine" }).click();
  await page.getByRole("button", { name: "Add SSH host" }).click();
  await page.waitForTimeout(1000);
  await page.mouse.click(80, 80);
  await page.waitForTimeout(1000);
  await capture(page, {
    name: "01-onboarding-ssh-after-backdrop-click",
    title: "Remote dialog after a click on the backdrop",
    screen: true,
    expectations: ["A click outside the dialog closes it"],
  });
});

function closePicker(): void {
  try {
    const dialog = execFileSync("xdotool", ["search", "--onlyvisible", "--name", "Select folder"], {
      encoding: "utf8",
    }).split("\n").filter(Boolean)[0];
    if (!dialog) return;
    const geo = Object.fromEntries(
      execFileSync("xdotool", ["getwindowgeometry", "--shell", dialog], { encoding: "utf8" })
        .split("\n").filter(Boolean).map((l) => l.split("=")),
    );
    // Cancel: left of Open at the dialog's bottom right (GDK_SCALE=2).
    const x = Number(geo.X) + Number(geo.WIDTH) - 280;
    const y = Number(geo.Y) + Number(geo.HEIGHT) - 48;
    execFileSync("xdotool", ["mousemove", String(x), String(y), "click", "1"]);
  } catch (error) {
    console.warn("qa: could not close the folder picker", error);
  }
}
