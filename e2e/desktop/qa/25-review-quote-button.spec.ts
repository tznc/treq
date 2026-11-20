// The Quote button on a review thread's comment in the Changes tab: a ghost
// icon button in the comment's bottom row, after the reactions (it used to
// float at the top-right, rendered below the card and clipped).
import { expect, test } from "@playwright/test";
import { capture } from "./capture.ts";
import { openTab, openWorkspace, settle } from "./helpers.ts";

test("review-quote-button: in the card's bottom row", async ({ page }) => {
  await openWorkspace(page, "feat/empty-event-message");
  await openTab(page, "Changes");
  await settle(page, 1500);
  await page.getByRole("button", { name: "packages/web/src/lib/client.ts", exact: true }).click();
  await settle(page, 1500);
  await page.getByRole("button", { name: "Show review thread" }).first().click();
  await settle(page, 800);
  await expect(page.getByRole("button", { name: "Quote" }).first()).toBeVisible();
  await capture(page, {
    name: "25-quote-01-bottom-row",
    title: "Quote button in the comment's bottom row",
    expectations: [
      "Each review comment's bottom row shows add-reaction, any reaction chips, then a quote-icon button",
      "Nothing sits at the card's top-right and nothing is clipped between cards",
    ],
  });
});
