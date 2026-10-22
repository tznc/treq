// The selected workspace row: only its background is tinted. The branch
// icon and title keep the colors of an unselected row.
import { test } from "@playwright/test";
import { capture } from "./capture.ts";
import { openWorkspace, ready, row, settle } from "./helpers.ts";

test.describe.configure({ mode: "serial" });

test("sidebar-selected-row: background tint only", async ({ page }) => {
  await ready(page);
  await settle(page, 3000);
  await openWorkspace(page, "feat/event-ingest");
  const box = await row(page, "feat/event-ingest").boundingBox();
  if (!box) throw new Error("row not drawn");
  await capture(page, {
    name: "15-sidebar-selected-row",
    title: "Selected workspace row next to unselected ones",
    clip: { x: 0, y: Math.max(0, box.y - 80), width: box.x + box.width + 8, height: 200 },
    expectations: [
      "feat/event-ingest has a tinted background",
      "Its branch icon and title use the same colors as the unselected rows around it",
    ],
  });
});
