// AI Review backlink: a review started from a GitHub PR shows a `#N` chip
// in its terminal header, beside the home-repo branch chip.
import { expect, test } from "@playwright/test";
import { capture } from "./capture.ts";
import { ready, settle } from "./helpers.ts";

const PRS_TAB = { x: 422, y: 62 };
const FIRST_ROW = { x: 700, y: 147 };

test("ai review backlink: the PR review's header links to the PR", async ({ page }) => {
  await ready(page);
  await page.getByRole("button", { name: "GitHub", exact: true }).click();
  await settle(page, 2000);
  await page.mouse.click(PRS_TAB.x, PRS_TAB.y);
  await settle(page, 1500);
  await page.mouse.click(FIRST_ROW.x, FIRST_ROW.y);
  await settle(page, 2500);
  await page.getByRole("button", { name: "Review changes" }).click();
  await settle(page, 3000);
  await page.getByRole("button", { name: "AI review" }).click();
  const chip = page.getByRole("link", { name: /\/pull\/418$/ });
  await expect(chip).toBeVisible({ timeout: 15_000 });
  await settle(page, 1500);
  await capture(page, {
    name: "24-ai-review-backlink-01-header",
    title: "AI Review header with the PR chip",
    expectations: [
      "The terminal header reads AI Review, then the home branch chip, then a GitHub-icon #418 chip",
      "The #418 chip text is legible on the dark header and not clipped",
    ],
  });
});
