// The workspace header's diff-size marker, right after the info icon.
import { expect, test } from "@playwright/test";
import { capture } from "./capture.ts";
import { openTab, openWorkspace, settle } from "./helpers.ts";

test("header-loc: diff size follows the info icon", async ({ page }) => {
  await openWorkspace(page, "feat/empty-event-message");
  await openTab(page, "Code");
  await expect(page.getByRole("button", { name: "Workspace details" })).toBeVisible();
  await settle(page, 2000);
  await capture(page, {
    name: "20-header-loc",
    title: "Workspace header with the diff-size marker",
    expectations: [
      "Header: title, info icon, then the +N ▬ -M diff marker",
      "Tab row right edge shows no diff marker",
    ],
  });
});
