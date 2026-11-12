// The selected workspace tab's count drops its pill and takes the label's
// colour, so it reads cleanly on the selected fill.
import { test } from "@playwright/test";
import { capture } from "./capture.ts";
import { openTab, openWorkspace } from "./helpers.ts";

test("tabs: the selected tab's count sits on its fill without a pill", async ({ page }) => {
  await openWorkspace(page, "feat/empty-event-message");
  await openTab(page, "Changes");
  await capture(page, {
    name: "21-tab-count-selected",
    title: "Changes selected: its count has no pill",
    expectations: [
      "The selected Changes tab shows its count as plain text in the label's colour",
      "The unselected Commits tab keeps its muted count",
    ],
  });
});
