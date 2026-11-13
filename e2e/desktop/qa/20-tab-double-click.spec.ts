// Double-clicking a workspace tab returns it to its base page: the Code tab
// leaves the file view for the overview.
import { expect, test } from "@playwright/test";
import { capture } from "./capture.ts";
import { mod, openTab, openWorkspace, settle } from "./helpers.ts";

const BRANCH = "feat/empty-event-message";

test("tabs: double-clicking Code returns to its overview", async ({ page }) => {
  await openWorkspace(page, BRANCH);
  await openTab(page, "Code");
  await page.keyboard.press(`${mod}+p`);
  const search = page.getByRole("textbox", { name: "Go to file…" });
  await expect(search).toBeVisible();
  await search.fill("README");
  await settle(page, 800);
  await page.keyboard.press("Enter");
  await settle(page, 1500);
  const goToFile = page.getByRole("button", { name: "Go to file" });
  await expect(goToFile).toHaveCount(0);
  await capture(page, {
    name: "20-tab-01-file-view",
    title: "Before: Code tab showing a file, with Back",
    expectations: ["The Code tab shows README.md in the file view with a Back row"],
  });

  await page.getByRole("tab", { name: "Code", exact: true }).first().dblclick();
  await expect(goToFile).toBeVisible();
  await settle(page, 1000);
  await capture(page, {
    name: "20-tab-02-code-home",
    title: "After double-clicking Code: the overview",
    expectations: [
      "The Code tab is back on its overview: Agent and Go to file row, stack panel",
      "No Back row",
    ],
  });
});
