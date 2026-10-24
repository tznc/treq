// The GitHub view (fed by the fake `gh`): issues and their detail, the
// new-issue form, pull requests with filters and search, a PR's detail,
// its checks popover and its review screen, and the new-PR form; then the
// Linear view with no API key.
//
// The Merge Queue tab never shows: the GPUI window's route sets
// `mergeQueue` to null (window/snapshots.rs, the feature flag is off).
//
// List rows and the view's own tabs are not observable; they are clicked
// at their place in the 1400x900 window.
import { expect, test, type Page } from "@playwright/test";
import { capture } from "./capture.ts";
import { dismissPopover, ready, settle } from "./helpers.ts";

test.describe.configure({ mode: "serial" });

const ISSUES_TAB = { x: 332, y: 62 };
const PRS_TAB = { x: 422, y: 62 };
const FIRST_ROW = { x: 700, y: 147 };

async function github(page: Page): Promise<void> {
  await ready(page);
  await page.getByRole("button", { name: "Github", exact: true }).click();
  await expect(page.getByRole("button", { name: "Github", selected: true })).toBeVisible();
  await settle(page, 2000);
}

/** Closes an open detail pane (its × at the top right). */
async function closeDetail(page: Page): Promise<void> {
  await page.mouse.click(1369, 26);
  await settle(page, 800);
}

test("github-linear: issues", async ({ page }) => {
  await github(page);
  await page.mouse.click(ISSUES_TAB.x, ISSUES_TAB.y);
  await settle(page, 1500);
  await capture(page, {
    name: "10-github-issues",
    title: "GitHub view: open issues",
    expectations: [
      "Repo name, Issues / Pull Requests tabs, Open/Closed/All filter, refresh and New",
      "Rows: title, number, Open badge and date aligned; chevrons on the right",
    ],
  });
  await page.mouse.click(382, 103); // Closed
  await settle(page, 1500);
  await capture(page, {
    name: "10-github-issues-closed",
    title: "GitHub view: closed issues",
    expectations: ["Closed issues (purple/grey badge) or an empty state"],
  });
  await page.mouse.click(323, 103); // Open
  await settle(page, 1200);
  await page.mouse.click(FIRST_ROW.x, FIRST_ROW.y);
  await settle(page, 2500);
  await capture(page, {
    name: "10-github-issue-detail",
    title: "GitHub issue detail",
    expectations: [
      "List narrows to the left; detail shows title, state, body and comments",
      "Create Workspace / Close Issue / comment box aligned",
    ],
  });
  await closeDetail(page);
  await page.getByRole("button", { name: "New", exact: true }).click();
  await settle(page, 1500);
  await capture(page, {
    name: "10-github-new-issue",
    title: "GitHub: New Issue form",
    screen: true,
    rest: false,
    expectations: ["Title and body fields, Cancel and Create aligned; focus in Title"],
  });
  await page.keyboard.press("Escape");
  await settle(page, 600);
  const cancel = page.getByRole("button", { name: "Cancel" });
  if ((await cancel.count()) > 0) await cancel.first().click();
  await settle(page, 600);
});

test("github-linear: pull requests", async ({ page }) => {
  await github(page);
  await page.mouse.click(PRS_TAB.x, PRS_TAB.y);
  await settle(page, 2000);
  await capture(page, {
    name: "10-github-prs",
    title: "GitHub view: open pull requests",
    expectations: [
      "Draft/Open/Closed/All filter, Filters, Search PRs, refresh and New in one row",
      "Rows: title, number, Open badge, author, head → base, date",
    ],
  });
  await page.getByRole("button", { name: "Filters" }).click();
  await settle(page, 1000);
  await capture(page, {
    name: "10-github-pr-filters",
    title: "Pull requests: Filters popover",
    screen: true,
    rest: false,
    expectations: ["Filter options (author, label, …) aligned under the button"],
  });
  await dismissPopover(page);
  await page.getByRole("textbox", { name: "Search PRs" }).fill("slack");
  await settle(page, 1500);
  await capture(page, {
    name: "10-github-pr-search",
    title: "Pull requests: search 'slack'",
    expectations: ["Only matching PRs remain (#421, #412)"],
  });
  await page.getByRole("textbox", { name: "Search PRs" }).fill("no-such-pr-zzz");
  await settle(page, 1500);
  await capture(page, {
    name: "10-github-pr-search-empty",
    title: "Pull requests: search with no match",
    expectations: ["An explicit empty state"],
  });
  await page.getByRole("textbox", { name: "Search PRs" }).fill("");
  await settle(page, 1500);

  await page.mouse.click(FIRST_ROW.x, FIRST_ROW.y);
  await expect(page.getByRole("button", { name: "Review changes" })).toBeVisible();
  await settle(page, 2000);
  await capture(page, {
    name: "10-github-pr-detail",
    title: "Pull request #418 detail",
    expectations: [
      "Title wraps beside Review changes / Open Workspace / Open in Web",
      "Checks list with status icons; finished checks show a duration, the pending typecheck none",
      "Comment box with Comment / Convert to Draft / Close PR",
    ],
  });
  await page.getByRole("button", { name: "2/4" }).click();
  await settle(page, 1200);
  await capture(page, {
    name: "10-github-pr-checks-popover",
    title: "Pull request: checks badge popover",
    screen: true,
    rest: false,
    expectations: ["Each check with status, name and link; failing first"],
  });
  await dismissPopover(page);
  await page.getByRole("textbox", { name: "Leave a comment..." }).fill("Looks good once the test passes");
  await settle(page, 400);
  await capture(page, {
    name: "10-github-pr-comment-typed",
    title: "Pull request: comment typed",
    expectations: ["Comment button enabled; text not clipped"],
  });
  await page.getByRole("textbox", { name: "Leave a comment..." }).fill("");

  await page.getByRole("button", { name: "Review changes" }).click();
  await settle(page, 3000);
  await capture(page, {
    name: "10-github-pr-review",
    title: "Pull request review screen",
    expectations: [
      "File list and diff of the PR; review bar to submit",
      "A way back to the PR list",
    ],
    issue: "'No changes to review' and '0 files' for a PR with three changed files (may be the fake gh)",
  });
  await page.keyboard.press("Escape");
  await settle(page, 800);
});

test("github-linear: new pull request", async ({ page }) => {
  await github(page);
  await page.mouse.click(PRS_TAB.x, PRS_TAB.y);
  await settle(page, 1500);
  await page.getByRole("button", { name: "New", exact: true }).click();
  await settle(page, 2000);
  await capture(page, {
    name: "10-github-new-pr",
    title: "GitHub: new pull request form",
    screen: true,
    rest: false,
    expectations: [
      "Head and base pickers, title and body fields",
      "Create / Cancel aligned; draft option visible",
    ],
  });
  await page.keyboard.press("Escape");
  await settle(page, 600);
  const cancel = page.getByRole("button", { name: "Cancel" });
  if ((await cancel.count()) > 0) await cancel.first().click();
});

test("github-linear: linear without an API key", async ({ page }) => {
  await ready(page);
  await page.getByRole("button", { name: "Linear", exact: true }).click();
  await settle(page, 2000);
  await expect(page.getByText(/Linear is not connected/)).toBeVisible();
  await capture(page, {
    name: "10-linear-no-key",
    title: "Linear view with no API key",
    expectations: [
      "Says Linear is not connected and how to connect it (API key or OAuth)",
      "An Open Settings button under the message; no spinner",
    ],
  });
  await page.getByRole("button", { name: "Open Settings" }).click();
  await expect(page.getByRole("tab", { name: "Integrations", selected: true })).toBeVisible();
  await settle(page, 1500);
  await capture(page, {
    name: "10-linear-no-key-settings",
    title: "Open Settings from the Linear view",
    expectations: ["Settings opens on the Integrations tab, with the Linear section"],
  });
});
