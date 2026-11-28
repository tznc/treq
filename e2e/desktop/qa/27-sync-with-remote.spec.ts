// "Sync with remote" on the home-row menu: a commit pushed to origin while
// the app is open reaches main, and the workspaces targeting main are
// rebased onto it. The home view's own fetch already lists the commit, so
// the rebase is what the sync is checked by.
import { execFileSync } from "node:child_process";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { expect, test } from "@playwright/test";
import { fixture } from "../fixture.ts";
import { ALERTING } from "../landing/seed.ts";
import { capture } from "./capture.ts";
import { home, openTab, openWorkspace, popupBox, row, settle, xPointer } from "./helpers.ts";

const SUBJECT = "docs: note pushed from another machine";

/** Pushes a commit to origin's main from another clone, as a teammate would. */
function pushBehindTheApp(): string {
  const { repo, home: homeDir } = fixture();
  const env = { ...process.env, HOME: homeDir };
  const clone = mkdtempSync(join(tmpdir(), "qa-origin-"));
  const run = (...args: string[]) => execFileSync("git", args, { cwd: clone, env, encoding: "utf8" });
  run("clone", "-q", join(dirname(repo), "origin.git"), ".");
  writeFileSync(join(clone, "NOTES.md"), "Pushed while the app was open.\n");
  run("add", ".");
  run("-c", "user.name=QA", "-c", "user.email=qa@example.com", "commit", "-q", "-m", SUBJECT);
  run("push", "-q", "origin", "main");
  return run("rev-parse", "HEAD").trim();
}

/** Whether `branch` has `commit` among its ancestors, read without touching any working copy. */
function descends(branch: string, commit: string): boolean {
  const { repo, home: homeDir } = fixture();
  const out = execFileSync(
    "jj",
    ["log", "--ignore-working-copy", "--no-graph", "-r", `${commit} & ::"${branch}"`, "-T", "commit_id"],
    { cwd: repo, env: { ...process.env, HOME: homeDir }, encoding: "utf8" },
  );
  return out.trim().length > 0;
}

test("sync-with-remote: the home menu brings origin's main into the repo", async ({ page }) => {
  await home(page);
  await openTab(page, "Commits");
  // Before the push: once main moves the home row shows a commit id, not "main".
  await row(page, "main").first().click({ button: "right" });
  const tip = pushBehindTheApp();
  await settle(page, 1000);
  expect(descends(ALERTING, tip)).toBe(false);
  await capture(page, {
    name: "27-01-home-menu",
    title: "Home-row menu of event-bus",
    screen: true,
    rest: false,
    expectations: [
      "The menu opens on the selected lorem-ipsum/event-bus row",
      "'Sync with remote' is the second item, right under 'Switch Branch...', with an icon",
      "A separator follows it, then the path items and 'Add Repository…'",
    ],
  });

  const menu = popupBox();
  if (!menu) throw new Error("no menu");
  // The menu's 4px padding, then the middle of the second 32px row.
  await xPointer(page, menu.x + 40, menu.y + 4 + 32 + 16, 1);
  // The menu closes, then the toast pops up once the sync is done.
  await expect.poll(() => popupBox() === null, { timeout: 5_000 }).toBe(true);
  await expect.poll(() => popupBox() !== null, { timeout: 30_000 }).toBe(true);
  await settle(page, 300);
  await capture(page, {
    name: "27-02-synced-toast",
    title: "After picking Sync with remote",
    screen: true,
    expectations: [
      "A success toast reads 'Synced with remote'",
      "The menu has closed",
    ],
  });

  await expect.poll(() => descends(ALERTING, tip), { timeout: 30_000 }).toBe(true);
  await expect(page.getByText(SUBJECT).first()).toBeVisible();
  await capture(page, {
    name: "27-03-home-commits-synced",
    title: "Home Commits after the sync",
    expectations: [
      `main's Commits list shows '${SUBJECT}' on top`,
      "The older seed commits follow it",
    ],
  });

  await openWorkspace(page, ALERTING);
  await openTab(page, "Commits");
  await settle(page, 1500);
  await capture(page, {
    name: "27-04-workspace-rebased",
    title: `${ALERTING} after the sync`,
    expectations: [
      `The workspace's own commits sit on top of '${SUBJECT}', the new main`,
      "No rebase-held or conflict alert shows",
    ],
  });
});
