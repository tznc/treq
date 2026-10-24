// TREQ-195 and TREQ-45: the Code tab's file list marks each top-level
// file's status and folds untouched files behind a toggle; an alert on the
// same tab shows the workspace setup script, its commands and its output.
import { execFileSync } from "node:child_process";
import { rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { expect, test } from "@playwright/test";
import { appBinary } from "../env.ts";
import { fixture } from "../fixture.ts";
import { workspaceDir } from "../landing/seed.ts";
import { capture } from "./capture.ts";
import { mod, openTab, openWorkspace, overlay, settle } from "./helpers.ts";

const BRANCH = "feat/empty-event-message";

test.describe.configure({ mode: "serial" });

/**
 * Gives the workspace a committed top-level file (blue) and an uncommitted
 * one (yellow) beside its conflicted `packages` (red). The commit takes the
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
    name: "21-file-status-01-changed-only",
    title: "Code tab: changed files only",
    expectations: [
      "packages (conflicted) has a red name and red pip; CHANGELOG.md a blue pip; NOTES.md a yellow pip",
      "README.md and .gitignore (untouched) are not listed",
      "A 'Show N unchanged files' row with a right chevron closes the list",
    ],
  });

  await show.click();
  await expect(page.getByRole("button", { name: "README.md", exact: true })).toBeVisible();
  await settle(page, 600);
  await capture(page, {
    name: "21-file-status-02-unchanged-shown",
    title: "Code tab: unchanged files shown",
    expectations: [
      "Changed entries stay on top, then a 'Hide unchanged files' row with a down chevron",
      "Untouched entries (.gitignore, README.md) follow below that row, greyed out and without a pip",
    ],
  });
  await page.getByRole("button", { name: "Hide unchanged files" }).click();
});

test("setup-output: Code tab setup alert", async ({ page, context }) => {
  const config = join(fixture().repo, ".treq", "config.yaml");
  writeFileSync(
    config,
    `setup_script: ${JSON.stringify(
      "echo installing deps\necho 'error: lockfile out of date' 1>&2\nexit 1",
    )}\n`,
  );
  try {
    // The workspace predates the setup script, so it reads as pending (which
    // disables the alert's re-run button); the palette runs it regardless.
    await openWorkspace(page, BRANCH);
    await page.keyboard.press(`${mod}+k`);
    const palette = await overlay(page, context);
    await palette.getByRole("textbox", { name: "Type a command or search..." }).fill("setup");
    await palette.getByRole("option", { name: "Re-run setup script" }).click();
    await settle(page, 3000);
    await page.keyboard.press("Escape");
    await openTab(page, "Code");
    const show = page.getByRole("button", { name: "Show output" });
    await expect(show).toBeVisible({ timeout: 30_000 });
    await settle(page, 1000);
    await capture(page, {
      name: "21-setup-output-01-alert",
      title: "Code tab: failed setup script alert, output hidden",
      expectations: [
        "A red alert near the top of the Code tab reads 'Workspace setup script failed' / 'Checks can still run.'",
        "Its actions are 'Show output' and 'Re-run setup script'; no commands or output are shown yet",
      ],
    });

    await show.click();
    await expect(page.getByRole("button", { name: "Hide output" })).toBeVisible();
    await settle(page, 1000);
    await capture(page, {
      name: "21-setup-output-02-expanded",
      title: "Code tab: setup alert with output shown",
      expectations: [
        "Inside the alert, a Commands block lists '$ echo installing deps', the stderr echo and '$ exit 1'",
        "An Output block shows 'installing deps' and '[stderr] error: lockfile out of date'",
        "The actions now read 'Hide output' and 'Re-run setup script'",
      ],
    });
  } finally {
    rmSync(config, { force: true });
  }
});
