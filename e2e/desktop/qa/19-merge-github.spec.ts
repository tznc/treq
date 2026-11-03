// TREQ-257 in the Merge Preview: merging locally or on GitHub, the landing
// panel, and a PR already merged on GitHub (Archive, not Merge…). (The demo's only stacked
// child has conflicts, so rollup is covered by the screen tests instead.)
// The fake gh serves the PRs (#401 open, #388 merged) and the repository's
// merge settings (squash by default, rebase off). Nothing is merged.
import { expect, test, type Page } from "@playwright/test";
import { capture } from "./capture.ts";
import { openWorkspace, settle } from "./helpers.ts";

test.describe.configure({ mode: "serial" });

async function openPreview(page: Page, branch: string): Promise<void> {
  await openWorkspace(page, branch);
  await page.getByRole("button", { name: "Merge…" }).click();
  await expect(page.getByRole("button", { name: "Back" })).toBeVisible();
  await settle(page, 2500);
}

test("merge github: locally or on GitHub", async ({ page }) => {
  await openPreview(page, "feat/keyvalues-cache");
  await expect(page.getByRole("button", { name: "On GitHub (#401)" })).toBeVisible();
  await capture(page, {
    name: "19-merge-github-01-local",
    title: "Open PR: merging locally",
    expectations: [
      "Locally / On GitHub (#401) toggle above the message, Locally selected",
      "A Push main to origin switch sits above Confirm merge",
      "What lands where: feat/keyvalues-cache flows down into main",
    ],
  });

  await page.getByRole("button", { name: "On GitHub (#401)" }).click();
  await expect(page.getByRole("button", { name: "Squash and merge on GitHub" })).toBeVisible();
  await settle(page, 800);
  await capture(page, {
    name: "19-merge-github-02-github",
    title: "Merging PR #401 on GitHub",
    expectations: [
      "The primary button reads Squash and merge on GitHub (the repo's default)",
      "The message is optional; no push switch",
      "The landing panel routes through PR #401 on GitHub and says main is synced from origin",
    ],
  });
  await page.getByRole("button", { name: "Back" }).click();
});

test("merge github: a PR already merged", async ({ page }) => {
  // #1254: once the PR merged on GitHub, the header offers Archive in
  // place of Merge…, so the work is not merged again.
  await openWorkspace(page, "feat/alerting-logs");
  await expect(page.getByRole("button", { name: "Archive", exact: true })).toBeVisible();
  await expect(page.getByRole("button", { name: "Merge…" })).toHaveCount(0);
  await settle(page, 1500);
  await capture(page, {
    name: "19-merge-github-03-merged",
    title: "PR #388 already merged on GitHub",
    expectations: [
      "The header's PR button reads Merged; an outline Archive button stands where Merge… was",
      "No Merge… button: the PR is no longer open",
    ],
  });
});
