// A workspace opened from a pull request shows the PR's title and body in
// one card, saves edits to the PR, or keeps them as a local draft until the
// next push; Sync brings the PR's text back. PR #415 has a markdown body in
// the fake gh, which remembers `gh pr edit`.
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { expect, test } from "@playwright/test";
import { fixture } from "../fixture.ts";
import { capture } from "./capture.ts";
import { home, openWorkspace, row, settle } from "./helpers.ts";

test.describe.configure({ mode: "serial" });

/** The sidebar names a workspace by its title, here PR #415's. */
const ROW = "feat(spool): fail readiness when the spool queue is unreachable";

/** The `gh pr edit` calls the app made. */
function prEdits(): string[][] {
  const log = join(fixture().root, "gh-calls.log");
  return readFileSync(log, "utf8")
    .split("\n")
    .filter(Boolean)
    .map((line) => JSON.parse(line) as string[])
    .filter((args) => args[0] === "pr" && args[1] === "edit");
}

test("pr-description-sync: synced, local draft, Sync, save to PR", async ({ page }) => {
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
  await expect(row(page, ROW)).toBeVisible({ timeout: 20_000 });

  await openWorkspace(page, ROW);
  const synced = page.getByText("Synced with PR #415");
  await expect(synced).toBeVisible({ timeout: 30_000 });
  await expect(page.getByText("From GitHub PR #415")).toBeHidden();
  await settle(page);
  await capture(page, {
    name: "20-pr-description-synced",
    title: "Title and description follow the PR",
    expectations: [
      "One card holds the title and, under it, the PR body as markdown (a 'Summary' heading, inline code, a two-item list), each with its edit pencil",
      "The card's footer, under a divider, shows a green check and 'Synced with PR #415'; no 'From GitHub PR #415' placeholder",
    ],
  });

  // A local draft: kept here, offered back with Sync.
  await page.getByRole("button", { name: "Edit description" }).click();
  await page.keyboard.press("Control+End");
  await page.keyboard.type("\n\nLocal note.");
  await page.getByRole("button", { name: "More save options" }).click();
  await expect(page.getByText("Save as local draft")).toBeVisible();
  await settle(page, 500);
  await capture(page, {
    name: "20-pr-description-save-menu",
    title: "Save's menu offers a local draft",
    expectations: [
      "Under the description editor, Cancel then a filled 'Save' split button with a chevron half",
      "A menu below it reads 'Save as local draft' with a line saying it goes to the pull request on the next push",
    ],
  });
  await page.getByText("Save as local draft").click();
  const sync = page.getByRole("button", { name: "Sync", exact: true });
  await expect(sync).toBeVisible({ timeout: 20_000 });
  await expect(page.getByText("Local draft, sent to PR #415 on your next push")).toBeVisible();
  await expect(synced).toBeHidden();
  expect(prEdits()).toEqual([]);
  await settle(page);
  await capture(page, {
    name: "20-pr-description-draft",
    title: "Local draft: Sync in the footer",
    expectations: [
      "The description ends with 'Local note.'",
      "The footer reads 'Local draft, sent to PR #415 on your next push' with a filled amber 'Sync' button at its right",
    ],
  });

  await sync.click();
  await expect(synced).toBeVisible({ timeout: 20_000 });
  await expect(page.getByText("Local note.")).toBeHidden();

  // Save goes to the PR, which keeps it.
  await page.getByRole("button", { name: "Edit description" }).click();
  await page.keyboard.press("Control+End");
  await page.keyboard.type("\n\nSent to GitHub.");
  await page.getByRole("button", { name: "Save", exact: true }).click();
  await expect(page.getByText("Sent to GitHub.")).toBeVisible({ timeout: 20_000 });
  await expect(synced).toBeVisible();
  await expect.poll(() => prEdits().length).toBe(1);
  expect(prEdits()[0].slice(0, 5)).toEqual(["pr", "edit", "415", "--repo", "lorem-ipsum/event-bus"]);
  // The next poll reads the PR's new body back: still synced, text kept.
  await settle(page, 3000);
  await expect(page.getByText("Sent to GitHub.")).toBeVisible();
  await expect(synced).toBeVisible();
});
