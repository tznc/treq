// "Sync with remote" on the home-row menu: a commit pushed to origin while
// the app is open reaches the home repository's main, and the workspaces
// targeting main are rebased onto it.
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
function pushBehindTheApp(): void {
  const { repo, home: homeDir } = fixture();
  const env = { ...process.env, HOME: homeDir };
  const clone = mkdtempSync(join(tmpdir(), "qa-origin-"));
  const run = (...args: string[]) => execFileSync("git", args, { cwd: clone, env, encoding: "utf8" });
  run("clone", "-q", join(dirname(repo), "origin.git"), ".");
  writeFileSync(join(clone, "NOTES.md"), "Pushed while the app was open.\n");
  run("add", ".");
  run("-c", "user.name=QA", "-c", "user.email=qa@example.com", "commit", "-q", "-m", SUBJECT);
  run("push", "-q", "origin", "main");
}

test("sync-with-remote: the home menu brings origin's main into the repo", async ({ page }) => {
  await home(page);
  await openTab(page, "Commits");
  pushBehindTheApp();
  await settle(page, 1500);
  await expect(page.getByText(SUBJECT)).toHaveCount(0);

  await row(page, "main").first().click({ button: "right" });
  await settle(page, 1000);
  await capture(page, {
    name: "27-01-home-menu",
    title: "Home-row menu of event-bus",
    screen: true,
    rest: false,
    expectations: [
      "'Sync with remote' is the second item, right under 'Switch Branch...', with an icon",
      "A separator follows it, then 'Copy relative path', 'Copy full path' and 'Open in…'",
    ],
  });

  const menu = popupBox();
  if (!menu) throw new Error("no menu");
  // The menu's 4px padding, then the middle of the second 32px row.
  await xPointer(page, menu.x + 40, menu.y + 4 + 32 + 16, 1);
  await settle(page, 2500);
  await capture(page, {
    name: "27-02-synced-toast",
    title: "After picking Sync with remote",
    screen: true,
    expectations: [
      "A success toast reads 'Synced with remote'",
      "The menu has closed",
    ],
  });

  await expect(page.getByText(SUBJECT).first()).toBeVisible({ timeout: 30_000 });
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
