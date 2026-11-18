// AI Review backlink: a review started from a GitHub PR shows a `#N` chip
// in its terminal header, beside the home-repo branch chip.
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { expect, test } from "@playwright/test";
import { fixture } from "../fixture.ts";
import { capture } from "./capture.ts";
import { ready, settle } from "./helpers.ts";

const PRS_TAB = { x: 422, y: 62 };
const FIRST_ROW = { x: 700, y: 147 };
const FILE = "packages/web/src/lib/search.ts";

// The fake `gh` serves PR #418's diff from this file; the review needs one.
const DIFF = `diff --git a/${FILE} b/${FILE}
index 1111111..2222222 100644
--- a/${FILE}
+++ b/${FILE}
@@ -1,3 +1,4 @@
 export function search(q: string) {
+  const query = q.trim();
   return q;
 }
`;

test("ai review backlink: the PR review's header links to the PR", async ({ page }) => {
  writeFileSync(join(fixture().root, "pr-diff-418.diff"), DIFF);
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
