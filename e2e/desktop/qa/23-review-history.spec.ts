// Review history: the Logs tab's Reviews source (empty in the demo repo)
// and Settings → Code Review's "Tune with agent" button.
import { expect, test } from "@playwright/test";
import { capture } from "./capture.ts";
import { openTab, openWorkspace, ready, settle, xPointer } from "./helpers.ts";

test.describe.configure({ mode: "serial" });

test("review history: Logs → Reviews", async ({ page }) => {
  await openWorkspace(page, "feat/keyvalues-cache");
  await openTab(page, "Logs");
  // The source picker is the toolbar's first button (see 06-logs).
  await page.mouse.click(340, 130);
  await settle(page, 900);
  await capture(page, {
    name: "23-review-history-source-menu",
    title: "Logs: source picker lists Reviews",
    screen: true,
    rest: false,
    expectations: ["Reviews listed among the sources, after Agent"],
  });
  // Menu rows: Checks, Agent, Reviews.
  await xPointer(page, 360, 150 + 2 * 28, 1);
  await settle(page, 1200);
  await expect(page.getByRole("textbox", { name: "Search reviews…" })).toBeVisible();
  await capture(page, {
    name: "23-review-history-empty",
    title: "Logs: Reviews source with no history",
    expectations: [
      "Reviews source selected; level filter and 'Search reviews…' in one row",
      "Empty state says what shows here and that history covers 30 days",
    ],
  });
});

test("review history: Settings → Tune with agent", async ({ page }) => {
  await ready(page);
  await page.getByRole("button", { name: "Settings", exact: true }).click();
  await page.getByRole("tab", { name: "Repository" }).click();
  await page.mouse.move(950, 500);
  const tune = page.getByRole("button", { name: "Tune with agent" });
  for (let i = 0; i < 6 && !(await tune.isVisible()); i++) {
    await page.mouse.wheel(0, 400);
    await settle(page, 500);
  }
  await expect(tune).toBeVisible();
  await capture(page, {
    name: "23-review-history-tune-button",
    title: "Settings: Code Review with Tune with agent",
    expectations: [
      "Tune with agent sits under Custom Review Prompt, an outline button",
      "Its help text reads on the same row, muted, without clipping",
    ],
  });
});
