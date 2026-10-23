// TREQ-195 and TREQ-45: the Code tab's file list marks each top-level
// file's status and folds untouched files behind a toggle; a PR's Treq
// checks open the workspace setup script's commands and output.
import { execFileSync } from "node:child_process";
import { rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { expect, test, type Page } from "@playwright/test";
import { appBinary } from "../env.ts";
import { fixture } from "../fixture.ts";
import { workspaceDir } from "../landing/seed.ts";
import { capture } from "./capture.ts";
import { mod, openTab, openWorkspace, overlay, ready, settle } from "./helpers.ts";

const BRANCH = "feat/empty-event-message";
const PRS_TAB = { x: 422, y: 62 };
const FIRST_ROW = { x: 700, y: 147 };

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
      "Untouched entries such as README.md now show, greyed out and without a pip",
      "The last row reads 'Hide unchanged files' with a down chevron",
    ],
  });
  await page.getByRole("button", { name: "Hide unchanged files" }).click();
});

async function prDetail(page: Page): Promise<void> {
  await ready(page);
  await page.keyboard.press("Escape");
  const github = page.getByRole("button", { name: "Github", exact: true });
  await expect(async () => {
    await github.click();
    await expect(page.getByRole("button", { name: "Github", selected: true })).toBeVisible({
      timeout: 2_000,
    });
  }).toPass({ timeout: 20_000 });
  await settle(page, 2000);
  await page.mouse.click(PRS_TAB.x, PRS_TAB.y);
  await settle(page, 1500);
  await page.mouse.click(FIRST_ROW.x, FIRST_ROW.y);
  await expect(page.getByRole("button", { name: "Review changes" })).toBeVisible();
  await settle(page, 1500);
}

test("setup-output: PR detail Treq checks", async ({ page, context }) => {
  const config = join(fixture().repo, ".treq", "config.yaml");
  writeFileSync(
    config,
    `setup_script: ${JSON.stringify(
      "echo installing deps\necho 'error: lockfile out of date' 1>&2\nexit 1",
    )}\n`,
  );
  try {
    // The workspace predates the setup script, so it reads as pending (which
    // disables the panel's re-run button); the palette runs it regardless.
    await openWorkspace(page, BRANCH);
    await page.keyboard.press(`${mod}+k`);
    const palette = await overlay(page, context);
    await palette.getByRole("textbox", { name: "Type a command or search..." }).fill("setup");
    await palette.getByRole("option", { name: "Re-run setup script" }).click();
    await settle(page, 3000);
    await prDetail(page);
    const open = page.getByRole("button", { name: "Setup output", expanded: true });
    await expect(open).toBeVisible({ timeout: 30_000 });
    await settle(page, 1500);
    await capture(page, {
      name: "21-setup-output-01-failed",
      title: "PR detail: failed setup script, output open",
      expectations: [
        "The setup row reads 'Workspace setup script failed. Checks can still run.' with a red warning icon and a down chevron",
        "A Commands block lists '$ echo installing deps', the stderr echo and '$ exit 1'",
        "An Output block shows 'installing deps' and '[stderr] error: lockfile out of date'",
      ],
    });

    await open.click();
    await expect(
      page.getByRole("button", { name: "Setup output", expanded: false }),
    ).toBeVisible();
    await settle(page, 600);
    await capture(page, {
      name: "21-setup-output-02-collapsed",
      title: "PR detail: setup output collapsed",
      expectations: [
        "The setup row is one line with a right chevron and the Re-run setup script button",
        "The commands and output blocks are gone; the rest of the Treq checks section follows directly below",
      ],
    });
  } finally {
    rmSync(config, { force: true });
  }
});
