// The workspace header's Merge… button while a conflict blocks the merge.
import { test } from "@playwright/test";
import { capture } from "./capture.ts";
import { openWorkspace, settle } from "./helpers.ts";

test("merge-disabled: header Merge button blocked by a conflict", async ({ page }) => {
  await openWorkspace(page, "feat/empty-event-message");
  await settle(page, 2500);
  await capture(page, {
    name: "20-merge-disabled",
    title: "Merge… blocked by a conflict",
    expectations: [
      "Merge… reads as disabled: fill, icon and label all muted together",
      "Icon and label share one colour",
    ],
  });
});
