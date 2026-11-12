// Multi-select menu: with a workspace open, Ctrl+clicking a second row
// highlights both, and right-clicking either opens the menu that acts on
// both (no archive bar under the list).
import { expect, test } from "@playwright/test";
import { capture } from "./capture.ts";
import { openWorkspace, ready, row, settle } from "./helpers.ts";

test.describe.configure({ mode: "serial" });

test("bulk-archive: right-click opens the multi-select menu", async ({ page }) => {
  await ready(page);
  await openWorkspace(page, "feat/keyvalues-cache");
  await row(page, "feat/alerting-logs").click({ modifiers: ["Control"] });
  await settle(page, 800);
  for (const name of ["feat/keyvalues-cache", "feat/alerting-logs"]) {
    await expect(page.getByRole("button", { name, exact: true, selected: true })).toBeVisible();
  }
  await expect(page.getByRole("button", { name: "Archive 2 workspaces" })).toHaveCount(0);
  await row(page, "feat/alerting-logs").click({ button: "right" });
  await settle(page, 1500);
  await capture(page, {
    name: "15-bulk-archive-menu",
    title: "Context menu on two selected workspaces",
    screen: true,
    rest: false,
    expectations: [
      "feat/keyvalues-cache and feat/alerting-logs are both highlighted, with no archive bar below",
      "The menu lists Copy branch names, Copy relative paths, Copy full paths, Open in… and Archive 2 workspaces",
    ],
  });
});
