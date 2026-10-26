// The workspace header's right group: checks, PR pill, sync and Merge share
// one control height, and the sync pip reads clearly in both themes.
import { type Page, test } from "@playwright/test";
import { capture } from "./capture.ts";
import { openWorkspace, ready, settle } from "./helpers.ts";

async function headerClip(page: Page) {
  const box = await page.getByRole("button", { name: "More workspace actions" }).boundingBox();
  if (!box) throw new Error("no more button");
  return { x: box.x + box.width - 520, y: box.y - 12, width: 532, height: box.height + 24 };
}

test("toolbar: header controls share one height", async ({ page }) => {
  await ready(page);
  await openWorkspace(page, "feat/empty-event-message");
  await settle(page, 1500);
  await capture(page, {
    name: "25-toolbar-heights",
    title: "Workspace header controls",
    clip: await headerClip(page),
    expectations: [
      "Checks, the PR pill, sync, Merge and the menu share one height and vertical centre",
      "The sync pip is a solid amber dot, not clipped and clear of the button border",
    ],
  });
});
