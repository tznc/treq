// Long branch names in the sidebar: a hovered workspace row truncates its
// title before the Start agent, Open shell and Stack on this… buttons
// instead of running under them. The sidebar is dragged narrow so the
// seed's stacked feat/empty-event-message overflows.
import { expect, test } from "@playwright/test";
import { capture } from "./capture.ts";
import { ready, row, settle } from "./helpers.ts";

test.describe.configure({ mode: "serial" });

const LONG = "feat/empty-event-message";

test("sidebar-truncation: long titles stop before the row actions", async ({ page }) => {
  await ready(page);
  const settings = await page.getByRole("button", { name: "Settings", exact: true }).boundingBox();
  const target = await row(page, LONG).boundingBox();
  if (!settings || !target) throw new Error("sidebar not drawn");
  // The resize handle is the sidebar's 4px right edge, 8px right of the
  // Settings button; drag it to 220px.
  const edge = settings.x + settings.width + 6;
  await page.mouse.move(edge, target.y - 60);
  await page.mouse.down();
  await page.mouse.move(220, target.y - 60, { steps: 8 });
  await page.mouse.up();
  await settle(page, 800);

  await row(page, LONG).hover();
  await settle(page, 1500);
  await expect(row(page, LONG).getByRole("button", { name: "Stack on this…" })).toBeVisible();
  await capture(page, {
    name: "14-sidebar-long-title-hover",
    title: "Hovered workspace row with a title too long for the narrow sidebar",
    screen: true,
    rest: false,
    expectations: [
      "The title ends in an ellipsis before the Start agent button",
      "No title text is drawn under or between the three action buttons",
    ],
  });
});
