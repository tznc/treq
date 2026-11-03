// workspace/reviews, create-pr-commit and merge.test.tsx against the real
// backend: a file changed in a workspace shows on its Changes tab, Commit
// commits it, and the merge preview merges the workspace into main.
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { expect, test } from "@playwright/test";
import { commitSubjects, fixture, git, goHome } from "../fixture.ts";
import { workspaceDir } from "../landing/seed.ts";

/** The e2e/first checkout: `.treq/workspaces/<id>-e2e-first`. */
function workspacePath(): string {
  return workspaceDir(fixture().repo, "e2e/first");
}

test("commits a workspace's changes from the Changes tab", async ({ page }) => {
  await goHome(page);
  const row = page.getByRole("button", { name: "e2e/first", exact: true });
  await row.click();
  writeFileSync(join(workspacePath(), "feature.txt"), "feature work\n");

  await page.getByRole("tab", { name: "Changes" }).click();
  const commit = page.getByRole("button", { name: /^Commit/ }).first();
  // Commit waits for a message as well as the changes.
  await page.getByRole("textbox", { name: "Message" }).fill("Add feature file");
  await expect(commit).toBeEnabled({ timeout: 20_000 });
  await commit.click();
  await expect
    .poll(commitSubjects, { timeout: 20_000 })
    .toContain("Add feature file");
});

test("merges the workspace from the merge preview", async ({ page }) => {
  await goHome(page);
  await page.getByRole("button", { name: "e2e/first", exact: true }).click();
  await page.getByRole("button", { name: "Merge…" }).click();
  await expect(page.getByRole("button", { name: "Back" })).toBeVisible();
  const confirm = page.getByTestId("merge-confirm");
  await expect(confirm).toBeEnabled({ timeout: 20_000 });
  await confirm.click();
  await expect
    .poll(() => git("log", "main", "--format=%s"), { timeout: 30_000 })
    .toContain("feature");
});
