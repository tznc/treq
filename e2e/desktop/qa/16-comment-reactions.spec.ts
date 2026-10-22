// Reactions on GitHub comments: the counts under issue #77's comments in
// the GitHub view, toggling one, the picker of GitHub's eight reactions,
// and the same row under a review thread's comment in the Changes tab.
//
// The fake `gh` reports octocat as having reacted 👍 to bob's comment and
// 🚀 to the review comment. List rows and diff rows are not observable, so
// those steps click their place in the 1400x900 window.
import { expect, test, type Page } from "@playwright/test";
import { capture } from "./capture.ts";
import { openTab, openWorkspace, ready, settle } from "./helpers.ts";

test.describe.configure({ mode: "serial" });

const ISSUES_TAB = { x: 332, y: 62 };
const FIRST_ROW = { x: 700, y: 147 };

async function issueDetail(page: Page): Promise<void> {
  await ready(page);
  await page.getByRole("button", { name: "Github", exact: true }).click();
  await settle(page, 2000);
  await page.mouse.click(ISSUES_TAB.x, ISSUES_TAB.y);
  await settle(page, 1500);
  await page.mouse.click(FIRST_ROW.x, FIRST_ROW.y);
  await settle(page, 2500);
}

test("comment-reactions: issue comments", async ({ page }) => {
  await issueDetail(page);
  const thumbs = page.getByRole("button", { name: "Thumbs up (3, you reacted)" });
  await expect(thumbs).toBeVisible();
  await expect(page.getByRole("button", { name: "Heart (1)" })).toBeVisible();
  await capture(page, {
    name: "16-reactions-01-issue-comments",
    title: "Issue comments with their reactions",
    expectations: [
      "Under bob's comment: an add-reaction button, then 👍 3, ❤️ 1 and 👀 2 chips",
      "The 👍 chip is highlighted blue (octocat reacted); the others are plain outlines",
      "Alice's comment has only the add-reaction button",
    ],
  });

  await page.getByRole("button", { name: "Heart (1)" }).click();
  await expect(page.getByRole("button", { name: "Heart (2, you reacted)" })).toBeVisible();
  await thumbs.click();
  await expect(page.getByRole("button", { name: "Thumbs up (2)" })).toBeVisible();
  await capture(page, {
    name: "16-reactions-02-toggled",
    title: "Joined ❤️, left 👍",
    expectations: [
      "❤️ reads 2 and is highlighted; 👍 reads 2 and is no longer highlighted",
    ],
  });

  await page.getByRole("button", { name: "Add reaction" }).first().click();
  await settle(page, 600);
  await capture(page, {
    name: "16-reactions-03-picker",
    title: "The reaction picker",
    expectations: [
      "A popover under the add button with GitHub's eight reactions in one row",
      "❤️ is tinted (octocat's reaction); the rest are plain",
    ],
  });
  await page.getByRole("button", { name: "Rocket", exact: true }).click();
  await expect(page.getByRole("button", { name: "Rocket (1, you reacted)" })).toBeVisible();
  await capture(page, {
    name: "16-reactions-04-picked",
    title: "🚀 added from the picker",
    expectations: ["The picker closed and a highlighted 🚀 1 chip follows 👀"],
  });
});

test("comment-reactions: review thread", async ({ page }) => {
  await openWorkspace(page, "feat/empty-event-message");
  await openTab(page, "Changes");
  await settle(page, 1500);
  await page.mouse.click(326, 383); // client.ts under Committed
  await settle(page, 1500);
  await page.getByText("Resolved").first().click();
  await settle(page, 800);
  await expect(page.getByRole("button", { name: "Rocket (1, you reacted)" })).toBeVisible();
  await capture(page, {
    name: "16-reactions-05-review-thread",
    title: "A review comment's reactions in the Changes tab",
    expectations: [
      "Under octocat's review comment: add-reaction button, 👍 2 and a highlighted 🚀 1",
    ],
  });
});
