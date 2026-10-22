// Starting an agent in a workspace whose folder was deleted outside the app.
// The agent must not run in a recreated empty folder; its column says why.
// The failure is painted text, which the app doesn't observe, so the
// capture is the check.
// Runs last: it removes the workspace's checkout for the rest of the run.
import { test } from "@playwright/test";
import { capture } from "./capture.ts";
import { openTab, openWorkspace, row, settle } from "./helpers.ts";

const BRANCH = "feat/empty-event-message";

test("deleted workspace: starting an agent fails", async ({ page }) => {
  await openWorkspace(page, BRANCH);
  await openTab(page, "Commits");
  await row(page, BRANCH).hover();
  await row(page, BRANCH).getByRole("button", { name: "Open shell" }).click();
  await settle(page, 2500);
  await page.mouse.click(1000, 780); // focus the new shell
  await page.keyboard.type('d="$PWD"; cd / && rm -rf "$d"; clear; ls "$d"\n');
  await settle(page, 1500);

  await row(page, BRANCH).hover();
  await row(page, BRANCH).getByRole("button", { name: "Start agent" }).click();
  await settle(page, 3000);
  await capture(page, {
    name: "15-deleted-workspace-agent",
    title: "Terminal pane: an agent started in a deleted workspace",
    expectations: [
      "The agent column reads 'Failed to start agent: Workspace folder no longer exists: <path>', wrapped and fully visible",
      "The shell beside it shows ls reporting the workspace folder was deleted",
    ],
  });
});
