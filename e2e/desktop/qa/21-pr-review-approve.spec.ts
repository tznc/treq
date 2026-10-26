// Approving a pull request from its review screen goes back to the PR's
// detail, which reads the PR again; a PR whose workspace has no Treq checks
// says so in one line under its checks. The fake `gh` signs in as octocat,
// so #421 (alice's) can be approved.
//
// List rows, the view's tabs and the finish popover's verdicts are not
// observable; they are clicked at their place in the 1400x900 window.
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { expect, test, type Page } from "@playwright/test";
import { fixture } from "../fixture.ts";
import { capture } from "./capture.ts";
import { ready, settle } from "./helpers.ts";

test.describe.configure({ mode: "serial" });

const PRS_TAB = { x: 422, y: 62 };
const ROW_418 = { x: 700, y: 147 };
const ROW_421 = { x: 700, y: 214 };
const ROW_401 = { x: 700, y: 482 };
const APPROVE = { x: 1064, y: 265 };

async function openPr(page: Page, at: { x: number; y: number }): Promise<void> {
  await ready(page);
  await page.getByRole("button", { name: "Github", exact: true }).click();
  await settle(page, 2000);
  await page.mouse.click(PRS_TAB.x, PRS_TAB.y);
  await settle(page, 1500);
  await page.mouse.click(at.x, at.y);
  await settle(page, 2500);
}

test("pr review approve: no treq checks note", async ({ page }) => {
  await openPr(page, ROW_401);
  await capture(page, {
    name: "21-pr-approve-01-no-treq-checks",
    title: "PR #401: its workspace has no Treq checks",
    expectations: [
      "Checks collapsed to one line: 3/3 passed, with Show all",
      "One muted line under it: No Treq checks configured for this repository.",
      "No Treq checks heading or panel",
    ],
  });
});

test("pr review approve: back on the PR detail", async ({ page }) => {
  await openPr(page, ROW_421);
  await page.getByRole("button", { name: "Review changes" }).click();
  await settle(page, 3000);
  await page.getByRole("button", { name: /^Finish review/ }).click();
  await settle(page, 1000);
  await capture(page, {
    name: "21-pr-approve-02-finish",
    title: "Finish your review on #421",
    expectations: ["The finish popover with Comment, Approve and Request changes"],
    screen: true,
  });
  await page.mouse.click(APPROVE.x, APPROVE.y);
  await settle(page, 500);
  // GitHub records the approval: the PR's next read reports it.
  writeFileSync(
    join(fixture().home, "fake-gh-prs.json"),
    JSON.stringify({ "fix/slack-retry": { reviewDecision: "APPROVED" } }),
  );
  await page.getByRole("button", { name: "Submit review" }).click();
  await expect(page.getByRole("button", { name: "Review changes" })).toBeVisible({ timeout: 10_000 });
  await settle(page, 1000);
  await capture(page, {
    name: "21-pr-approve-03-back-on-pr",
    title: "After approving: back on #421's detail",
    expectations: [
      "PR #421's detail, not the review screen",
      "A green Approved badge beside Open",
      "A Review submitted toast reading #421: Approve",
    ],
    screen: true,
  });
});

test("pr review approve: collapsed checks", async ({ page }) => {
  await openPr(page, ROW_418);
  await capture(page, {
    name: "21-pr-approve-04-checks-collapsed",
    title: "PR #418: checks collapsed",
    expectations: [
      "Summary line: 2/4 passed · 1 failed · 1 running",
      "Only test (failed) and typecheck (running) are listed",
    ],
  });
  await page.getByRole("button", { name: "Show all" }).click();
  await settle(page, 600);
  await capture(page, {
    name: "21-pr-approve-05-checks-expanded",
    title: "PR #418: every check",
    expectations: ["All four checks listed, failing first", "The toggle reads Collapse"],
  });
});
