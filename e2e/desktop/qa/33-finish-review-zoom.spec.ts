// The Changes tab's "Finish your review" popover above 100% UI zoom: it
// opens under Finish review with right edges aligned, instead of flipping
// to the left of the button. Runs on the alerting workspace's uncommitted
// edits (qa/launch.ts).
import { expect, test } from "@playwright/test";
import { capture } from "./capture.ts";
import { mod, openTab, openWorkspace, settle } from "./helpers.ts";

const ALERTING = "feat/alerting-logs";

test("finish review: popover stays under its button at 125% zoom", async ({ page }) => {
  await openWorkspace(page, ALERTING);
  await openTab(page, "Changes");
  await settle(page, 1500);
  // The first added line's gutter (not observable, so by place in the
  // 1400x900 window), then Left opens the comment box.
  await page.mouse.click(600, 277);
  await page.keyboard.press("ArrowLeft");
  await page.getByRole("textbox", { name: "Add a comment…" }).fill("Nit");
  await page.getByRole("button", { name: "Add comment", exact: true }).click();
  // 100% to 125% in 5% steps.
  for (let i = 0; i < 5; i++) await page.keyboard.press(`${mod}+=`);
  try {
    await settle(page, 1000);
    const button = page.getByRole("button", { name: "Finish review" });
    await button.click();
    const popover = page.getByRole("dialog", { name: "Finish your review" });
    await expect(popover).toBeVisible();
    await settle(page, 400);
    const [b, p] = [await button.boundingBox(), await popover.boundingBox()];
    expect(b && p && Math.abs(b.x + b.width - (p.x + p.width))).toBeLessThan(2);
    expect(b && p && p.y >= b.y + b.height).toBe(true);
    await capture(page, {
      name: "33-finish-review-01-zoom-125",
      title: "Finish your review popover at 125% zoom",
      expectations: [
        "The popover opens directly under Finish review, its right edge lined up with the button's",
        "It does not sit under Start review or Discard pending comments…",
      ],
    });
    await page.getByRole("button", { name: "Close" }).click();
    await page.getByRole("button", { name: "Discard pending comments…" }).click();
    await page.getByRole("button", { name: "Discard comments" }).click();
  } finally {
    for (let i = 0; i < 5; i++) await page.keyboard.press(`${mod}+-`);
    await settle(page, 500);
  }
});
