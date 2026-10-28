// A drag held past the diff's bottom edge scrolls it and keeps selecting.
// Uses the alerting workspace's committed 642-line events.json (qa/launch.ts).
import { test } from "@playwright/test";
import { capture } from "./capture.ts";
import { openTab, openWorkspace, settle } from "./helpers.ts";

const GUTTER_X = 600;

test("diff drag autoscroll", async ({ page }) => {
  await openWorkspace(page, "feat/alerting-logs");
  await openTab(page, "Changes");
  await settle(page, 1500);
  await page.getByRole("button", { name: "View changes" }).click();
  await settle(page, 1500);
  const row = await page.locator('[data-debug-selector^="gutter-fixtures/events.json-0-"]').first().boundingBox().catch(() => null);
  const y0 = row ? row.y + row.height / 2 : 800;
  await page.mouse.move(GUTTER_X, y0);
  await page.mouse.down();
  await page.mouse.move(GUTTER_X, 880, { steps: 10 });
  await page.mouse.move(GUTTER_X, 899, { steps: 2 });
  await settle(page, 2500);
  await capture(page, {
    name: "26-drag-01-autoscrolled",
    title: "Holding a gutter drag at the bottom edge scrolls the diff",
    expectations: [
      "The diff has scrolled well into events.json: its file header is off screen",
      "The blue selection band runs unbroken down to the bottom edge",
    ],
  });
  await page.mouse.up();
});
