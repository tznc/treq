// GitHub review suggestions in the Changes tab: a thread whose comment holds
// a ```suggestion block, from the fake `gh` (PR #418).
import { expect, test } from "@playwright/test";
import { capture } from "./capture.ts";
import { openTab, openWorkspace, settle } from "./helpers.ts";

test("apply-suggestion: github thread", async ({ page }) => {
  await openWorkspace(page, "feat/empty-event-message");
  await openTab(page, "Changes");
  await settle(page, 1500);
  await page.getByRole("button", { name: "packages/web/src/lib/client.ts", exact: true }).click();
  await settle(page, 1500);
  await expect(page.getByText("Trim before checking length:")).toBeVisible();
  await capture(page, {
    name: "21-apply-suggestion-01-thread",
    title: "A GitHub suggestion thread",
    expectations: [
      "zoe's comment on line 5 shows the suggestion",
      "The thread is unresolved",
      "Baseline: no Apply or Apply & commit button",
    ],
  });
});
