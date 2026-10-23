// A workspace opened from a pull request shows the PR's body as its
// description and follows it until edited locally; Resync brings it back.
// PR #415 has a markdown body in the fake gh.
import { expect, test } from "@playwright/test";
import { capture } from "./capture.ts";
import { home, openWorkspace, row, settle } from "./helpers.ts";

test.describe.configure({ mode: "serial" });

const BRANCH = "feat/spool-readiness";

test("pr-description-sync: body copied in, edited, resynced", async ({ page }) => {
  await home(page);
  await page.getByRole("button", { name: "New workspaces" }).click();
  await expect(page.getByRole("heading", { name: "New Workspaces" })).toBeVisible();
  await page.getByLabel("Search pull requests").fill("spool");
  await page
    .getByRole("checkbox", {
      name: "#415 feat(spool): fail readiness when the spool queue is unreachable",
    })
    .click();
  await page.getByRole("button", { name: /^Create/ }).click();
  await expect(page.getByRole("heading", { name: "New Workspaces" })).toBeHidden({
    timeout: 30_000,
  });
  await expect(row(page, BRANCH)).toBeVisible({ timeout: 20_000 });

  await openWorkspace(page, BRANCH);
  await expect(page.getByText("Synced from PR #415")).toBeVisible({ timeout: 30_000 });
  await expect(page.getByText("From GitHub PR #415")).toBeHidden();
  await settle(page);
  await capture(page, {
    name: "20-pr-description-synced",
    title: "Description follows the PR body",
    expectations: [
      "Under the title, the description card renders the PR body as markdown: a 'Summary' heading, inline code and a two-item list",
      "A small muted 'Synced from PR #415' line sits at the bottom of the card; no 'From GitHub PR #415' placeholder",
    ],
  });

  await page.getByRole("button", { name: "Edit description" }).click();
  await page.keyboard.press("Control+End");
  await page.keyboard.type("\n\nLocal note.");
  await page.getByRole("button", { name: "Save" }).click();
  await expect(page.getByText("Edited locally")).toBeVisible({ timeout: 20_000 });
  await settle(page);
  await capture(page, {
    name: "20-pr-description-edited",
    title: "Edited locally: syncing stops, Resync offered",
    expectations: [
      "The description ends with 'Local note.'",
      "The card's last line reads 'Edited locally' beside a ghost 'Resync from PR #415' button",
    ],
  });

  await page.getByRole("button", { name: "Resync from PR #415" }).click();
  await expect(page.getByText("Synced from PR #415")).toBeVisible({ timeout: 20_000 });
  await expect(page.getByText("Local note.")).toBeHidden();
});
