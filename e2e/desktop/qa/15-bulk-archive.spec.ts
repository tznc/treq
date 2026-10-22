// Bulk archive count: with a workspace open, Ctrl+clicking a second row
// highlights both, and the bar counts both.
import { expect, test } from "@playwright/test";
import { capture } from "./capture.ts";
import { openWorkspace, ready, row, settle } from "./helpers.ts";

test.describe.configure({ mode: "serial" });

test("bulk-archive: Ctrl+click counts the open workspace", async ({ page }) => {
  await ready(page);
  await openWorkspace(page, "feat/keyvalues-cache");
  await row(page, "feat/alerting-logs").click({ modifiers: ["Control"] });
  await settle(page, 800);
  for (const name of ["feat/keyvalues-cache", "feat/alerting-logs"]) {
    await expect(page.getByRole("button", { name, exact: true, selected: true })).toBeVisible();
  }
  await capture(page, {
    name: "15-bulk-archive-ctrl-click",
    title: "Open workspace plus a Ctrl+clicked row",
    expectations: [
      "feat/keyvalues-cache and feat/alerting-logs are both highlighted",
      "The bulk archive bar reads 'Archive 2 workspaces'",
    ],
  });
  await expect(page.getByRole("button", { name: "Archive 2 workspaces" })).toBeVisible();
});
