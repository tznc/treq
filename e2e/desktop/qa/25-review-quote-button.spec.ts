// The Quote button on a review thread's comment in the Changes tab: hidden
// until the comment is hovered, then a ghost icon button inside the card's
// top-right corner (it used to render below the card, clipped).
import { expect, test } from "@playwright/test";
import { capture } from "./capture.ts";
import { openTab, openWorkspace, settle, xPointer } from "./helpers.ts";

test("review-quote-button: inside the comment card", async ({ page }) => {
  await openWorkspace(page, "feat/empty-event-message");
  await openTab(page, "Changes");
  await settle(page, 1500);
  await page.getByRole("button", { name: "packages/web/src/lib/client.ts", exact: true }).click();
  await settle(page, 1500);
  await page.getByRole("button", { name: "Show review thread" }).first().click();
  await settle(page, 800);
  const quote = page.getByRole("button", { name: "Quote" }).first();
  await expect(quote).toBeVisible();
  await capture(page, {
    name: "25-quote-01-idle",
    title: "Review comment, not hovered",
    expectations: [
      "The review comment card shows author, body and reactions",
      "No Quote button is visible and nothing is clipped between cards",
    ],
  });

  const text = await page.getByText("JSON.stringify is the right fallback").boundingBox();
  if (!text) throw new Error("no comment text");
  await xPointer(page, text.x + text.width + 200, text.y);
  await settle(page, 600);
  await capture(page, {
    name: "25-quote-02-hover",
    title: "Quote button on hover",
    expectations: [
      "A quote-icon button sits fully inside the comment card's top-right corner",
      "It is a ghost icon button (subtle hover background, rounded), not clipped",
    ],
  });
});
