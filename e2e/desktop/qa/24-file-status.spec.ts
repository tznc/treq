// TREQ-195: the Code tab's file list marks each top-level file's status and
// folds untouched files behind a toggle, below the changed ones.
import { execFileSync } from "node:child_process";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { expect, test } from "@playwright/test";
import { appBinary } from "../env.ts";
import { fixture } from "../fixture.ts";
import { workspaceDir } from "../landing/seed.ts";
import { capture } from "./capture.ts";
import { openTab, openWorkspace, settle } from "./helpers.ts";

const BRANCH = "feat/empty-event-message";

test.describe.configure({ mode: "serial" });

/**
 * Gives the workspace a committed top-level file (blue) and an uncommitted
 * one (yellow) beside its conflicted `packages` (red pip). The commit takes the
 * workspace's uncommitted edits with it, so this spec runs after the others.
 */
function seedStatuses(): void {
  const { repo, home } = fixture();
  const dir = workspaceDir(repo, BRANCH);
  writeFileSync(join(dir, "CHANGELOG.md"), "# Changelog\n\n- Handle empty event messages\n");
  execFileSync(appBinary, ["commit", BRANCH, "-m", "docs: add a changelog"], {
    cwd: repo,
    env: { ...process.env, HOME: home, TREQ_APP_DATA_DIR: join(home, ".local/share/com.treq") },
    stdio: "pipe",
  });
  writeFileSync(join(dir, "NOTES.md"), "Follow up on Slack retries.\n");
}

test("file-status: Code tab file list", async ({ page }) => {
  seedStatuses();
  await openWorkspace(page, BRANCH);
  await openTab(page, "Code");
  const show = page.getByRole("button", { name: /^Show \d+ unchanged files?$/ });
  await expect(show).toBeVisible();
  await expect(page.getByRole("button", { name: "README.md", exact: true })).toHaveCount(0);
  await settle(page, 800);
  await capture(page, {
    name: "24-file-status-01-changed-only",
    title: "Code tab: changed files only",
    expectations: [
      "packages (conflicted) has a neutral name and a red pip; CHANGELOG.md a blue pip; NOTES.md a yellow pip",
      "README.md and .gitignore (untouched) are not listed",
      "A 'Show N unchanged files' row with a right chevron closes the list",
    ],
  });

  await show.click();
  await expect(page.getByRole("button", { name: "README.md", exact: true })).toBeVisible();
  await settle(page, 600);
  await capture(page, {
    name: "24-file-status-02-unchanged-shown",
    title: "Code tab: unchanged files shown",
    expectations: [
      "Changed entries stay on top, then a 'Hide unchanged files' row with a down chevron",
      "Untouched entries (.gitignore, README.md) follow below that row, greyed out and without a pip",
    ],
  });
  await page.getByRole("button", { name: "Hide unchanged files" }).click();
});
