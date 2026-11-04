// Approving a pull request from its review screen goes back to the PR's
// detail, which reads the PR again; a PR whose workspace has no Treq checks
// says so in one line under its checks. The fake `gh` signs in as octocat,
// so #421 (alice's) can be approved.
//
// List rows are list items named by their title, and the finish popover's
// verdicts radio buttons named by their label.
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { expect, test, type Page } from "@playwright/test";
import { fixture } from "../fixture.ts";
import { capture } from "./capture.ts";
import { ready, settle } from "./helpers.ts";

test.describe.configure({ mode: "serial" });

const ROW_418 = "feat: handle empty event messages";
const ROW_421 = "fix: retry Slack webhooks on 429";
const ROW_415 = "feat(spool): fail readiness when the spool queue is unreachable";
const ROW_401 = "feat: keyvalues cache";

async function openPr(page: Page, title: string): Promise<void> {
  await ready(page);
  await page.getByRole("button", { name: "GitHub", exact: true }).click();
  await settle(page, 2000);
  await page.getByRole("tab", { name: "Pull requests" }).click();
  await settle(page, 1500);
  await page.getByRole("listitem", { name: title, exact: true }).click();
  await settle(page, 2500);
}

test("pr review approve: no treq checks note", async ({ page }) => {
  await openPr(page, ROW_401);
  await capture(page, {
    name: "21-pr-approve-01-no-treq-checks",
    title: "PR #401: its workspace has no Treq checks",
    expectations: [
      "Checks collapsed to one line: 3/3 passed, with Show all",
      "An empty state under it: 'No Treq checks' and why (no checks in .treq/workflows/)",
      "No checks list or Run buttons",
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
  await page.getByRole("radio", { name: "Approve", exact: true }).click();
  await settle(page, 500);
  // GitHub records the approval: the PR's next read reports it.
  writeFileSync(
    join(fixture().home, "fake-gh-prs.json"),
    JSON.stringify({
      "fix/slack-retry": {
        reviewDecision: "APPROVED",
        reviews: [{ author: { login: "octocat" }, state: "APPROVED", submittedAt: "2026-08-13T09:30:00Z" }],
      },
    }),
  );
  await page.getByRole("button", { name: "Submit review" }).click();
  await expect(page.getByRole("button", { name: "Review changes" })).toBeVisible({ timeout: 10_000 });
  await settle(page, 1000);
  await capture(page, {
    name: "21-pr-approve-03-back-on-pr",
    title: "After approving: back on #421's detail",
    expectations: [
      "Back on #421's detail with a green Approved badge beside Open",
      "Timeline: a check, octocat's avatar (or initial), octocat approved these changes · date",
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

test("pr review approve: a comment also returns", async ({ page }) => {
  await openPr(page, ROW_415);
  await page.getByRole("button", { name: "Review changes" }).click();
  await settle(page, 3000);
  await page.getByRole("button", { name: /^Finish review/ }).click();
  await settle(page, 800);
  await page.keyboard.type("Looks good, one question on the timeout.");
  await page.getByRole("button", { name: "Submit review" }).click();
  await expect(page.getByRole("button", { name: "Review changes" })).toBeVisible({ timeout: 10_000 });
  await settle(page, 1000);
  await capture(page, {
    name: "21-pr-approve-06-comment-back-on-pr",
    title: "After commenting: back on #415's detail",
    expectations: [
      "PR #415's detail, not the review screen",
      "A Review submitted toast reading #415: Comment",
    ],
    screen: true,
  });
});
