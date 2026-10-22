// The selected workspace row: a tinted background and full-contrast title
// text. Neither the branch icon nor the title turns blue.
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
      "Its title is full-contrast foreground text, not blue; its branch icon keeps the unselected color",
    ],
  });
});
