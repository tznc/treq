// PR review: starting an AI review keeps the PR on screen; the review's
// agent starts in the background in the home repository.
import { expect, test } from "@playwright/test";
import { capture } from "./capture.ts";
import { ready, settle } from "./helpers.ts";

const PRS_TAB = { x: 422, y: 62 };
const FIRST_ROW = { x: 700, y: 147 };

test("pr review: AI review keeps the PR on screen", async ({ page }) => {
  await ready(page);
  await page.getByRole("button", { name: "GitHub", exact: true }).click();
  await settle(page, 2000);
  await page.mouse.click(PRS_TAB.x, PRS_TAB.y);
  await settle(page, 1500);
  await page.mouse.click(FIRST_ROW.x, FIRST_ROW.y);
  await settle(page, 2500);
  await page.getByRole("button", { name: "Review changes" }).click();
  await settle(page, 3000);
  const aiReview = page.getByRole("button", { name: "AI review" });
  await expect(aiReview).toBeVisible();
  await capture(page, {
    name: "30-pr-ai-review-01-before",
    title: "PR review screen before starting an AI review",
    expectations: ["PR #418 review screen with its diff and the AI review button"],
  });

  await aiReview.click();
  await settle(page, 3000);
  await expect(page.getByRole("button", { name: "AI review" })).toBeVisible();
  await expect(page.getByRole("button", { name: /Finish review/ })).toBeVisible();
  await capture(page, {
    name: "30-pr-ai-review-02-after",
    title: "After clicking AI review: still on the PR",
    expectations: [
      "Still the PR #418 review screen, not the home repository",
      "The diff and Finish review button are still shown",
    ],
  });
});
