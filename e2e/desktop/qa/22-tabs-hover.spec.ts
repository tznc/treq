// The workspace view's tab strip (Code / Commits / Changes / Logs) with a
// tab under the pointer: the hover fill must stay visible on the muted
// strip, and the active tab has no shadow.
import { test } from "@playwright/test";
import { capture } from "./capture.ts";
import { openWorkspace, settle } from "./helpers.ts";

test("tabs-hover: workspace view tabs", async ({ page }) => {
  await openWorkspace(page, "feat/keyvalues-cache");
  const tab = page.getByRole("tab", { name: "Commits" });
  const box = await tab.boundingBox();
  if (!box) throw new Error("tab not drawn");
  await tab.hover();
  await settle(page, 600);
  await capture(page, {
    name: "22-tabs-hover-workspace",
    title: "Workspace tabs with Commits hovered",
    rest: false,
    clip: { x: box.x - 200, y: box.y - 16, width: 700, height: box.height + 32 },
    expectations: [
      "Hovered tab shows a visible fill distinct from the strip; active tab has no shadow",
    ],
  });
});
