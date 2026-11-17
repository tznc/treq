// The shell caches the screen beside the sidebar: a sidebar frame replays
// the screen's last drawing. These captures check that the screen still
// follows what changes it: a sidebar hover leaves the Changes diff drawn,
// a click in the screen and a workspace switch draw the screen again.
import { expect, test } from "@playwright/test";
import { ALERTING, CACHE } from "../landing/seed.ts";
import { capture } from "./capture.ts";
import { openTab, openWorkspace, row, settle, xPointer } from "./helpers.ts";

/** alerting.ts in feat/alerting-logs's Uncommitted list (see 05-changes). */
const SELECTED_ROW = { x: 330, y: 283 };

test("cached-screen: the screen follows sidebar frames and its own changes", async ({ page }) => {
  await openWorkspace(page, ALERTING);
  await openTab(page, "Changes");
  await page.mouse.click(SELECTED_ROW.x, SELECTED_ROW.y);
  await settle(page);
  const cache = await row(page, CACHE).boundingBox();
  expect(cache).not.toBeNull();
  // The pointer crosses the sidebar's rows: sidebar-only frames. GPUI
  // hovers follow the real X pointer.
  for (const dy of [-60, -30, 0]) {
    await xPointer(page, cache!.x + 40, cache!.y + cache!.height / 2 + dy);
  }
  await settle(page, 500);
  await capture(page, {
    name: "21-cached-screen-01-sidebar-hover",
    title: "Sidebar hover beside the Changes tab",
    expectations: [
      "After the sidebar-only hover frames, the Changes tab shows the file list and both diffs fully drawn",
      "Only the selected sidebar row is highlighted (capture parks the pointer)",
    ],
  });
  await page.getByRole("tab", { name: "Commits" }).click();
  await expect(page.getByRole("tab", { name: "Commits", selected: true })).toBeVisible();
  await settle(page);
  await capture(page, {
    name: "21-cached-screen-02-own-change",
    title: "A tab change inside the cached screen",
    expectations: ["The Commits tab is selected and its commit list is drawn, not the old Changes diff"],
  });
  await row(page, CACHE).click();
  await expect(page.getByRole("button", { name: CACHE, exact: true, selected: true })).toBeVisible();
  await settle(page, 1500);
  await capture(page, {
    name: "21-cached-screen-03-switched",
    title: "Switching workspace from the sidebar",
    expectations: [
      `The header names ${CACHE}`,
      "The screen shows that workspace's content, not feat/alerting-logs's",
    ],
  });
});
