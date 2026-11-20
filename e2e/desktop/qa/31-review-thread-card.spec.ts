// A GitHub review thread with a reply on PR #418's client.ts: the comment
// and its reply draw as one card, split by a rule, not one card each.
import { expect, test } from "@playwright/test";
import { capture } from "./capture.ts";
import { openTab, openWorkspace, settle } from "./helpers.ts";

test("review thread: replies share one card", async ({ page }) => {
  await openWorkspace(page, "feat/empty-event-message");
  await openTab(page, "Changes");
  await settle(page, 1500);
  await page.mouse.click(326, 320); // client.ts under Committed
  await settle(page, 1500);
  await page.getByRole("button", { name: "Show review thread" }).click();
  await expect(page.getByRole("button", { name: "Hide review thread" })).toBeVisible();
  await settle(page, 800);
  await capture(page, {
    name: "31-review-thread-card-01-replies",
    title: "Review thread with a reply, unfolded",
    expectations: [
      "octocat's comment and hubot's reply sit in one bordered card",
      "A thin rule separates the reply from the comment above it, with no gap",
      "Both authors, dates and bodies are readable; nothing clipped",
    ],
  });
});
