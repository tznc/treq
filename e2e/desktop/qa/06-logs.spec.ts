// The Logs tab: its empty state (the demo repo has no check runs), the
// source and level filters, a typed query and the Logs Explorer view.
import { test } from "@playwright/test";
import { capture } from "./capture.ts";
import { openTab, openWorkspace, settle } from "./helpers.ts";

test("logs: empty state, filters, query and explorer", async ({ page }) => {
  await openWorkspace(page, "feat/keyvalues-cache");
  await openTab(page, "Logs");
  await capture(page, {
    name: "06-logs-empty",
    title: "Logs tab: no check logs yet",
    expectations: [
      "Checks source picker, All Levels filter and search field aligned in one row",
      "Empty-state text tells how to populate the table",
      "Browse / Logs Explorer toggle at the top right",
    ],
  });
  // The source and level pickers are the first two unnamed buttons of the
  // toolbar: their place under the tab row.
  await page.mouse.click(340, 130);
  await settle(page, 900);
  await capture(page, {
    name: "06-logs-source-menu",
    title: "Logs: source picker open",
    screen: true,
    rest: false,
    expectations: ["Sources (Checks, …) listed under the trigger; the current one marked"],
  });
  await page.keyboard.press("Escape");
  await settle(page, 400);
  await page.mouse.click(360, 183);
  await settle(page, 900);
  await capture(page, {
    name: "06-logs-level-menu",
    title: "Logs: level filter open",
    screen: true,
    rest: false,
    expectations: ["Levels listed and aligned, including a way back to All Levels"],
  });
  await page.keyboard.press("Escape");
  await settle(page, 400);

  const search = page.getByRole("textbox", { name: "Search all runs…" });
  await search.fill("level:error timeout");
  await settle(page, 1200);
  await capture(page, {
    name: "06-logs-query",
    title: "Logs: a typed query",
    expectations: ["Query text visible; table shows a no-match state, not a spinner"],
  });
  await search.fill("");

  await page.getByRole("button", { name: "Logs explorer" }).click();
  await settle(page, 1500);
  await capture(page, {
    name: "06-logs-explorer",
    title: "Logs Explorer view",
    expectations: [
      "Explorer layout (query editor, results) renders without overlap",
      "Explorer segment selected in the toggle",
    ],
  });
  const templates = page.getByRole("button", { name: "Templates" });
  if ((await templates.count()) > 0) {
    await templates.click();
    await settle(page, 900);
    await capture(page, {
      name: "06-logs-explorer-templates",
      title: "Logs Explorer: query templates menu",
      screen: true,
      rest: false,
      expectations: ["Template names readable and aligned with the trigger"],
    });
    await page.keyboard.press("Escape");
    await settle(page, 400);
  }
  const run = page.getByRole("button", { name: "Run query" });
  if ((await run.count()) > 0) {
    await run.click();
    await settle(page, 2000);
    await capture(page, {
      name: "06-logs-explorer-results",
      title: "Logs Explorer: query run on an empty logs view",
      expectations: ["An empty result table or a clear 'no rows' message, no error"],
    });
  }
  await page.getByRole("button", { name: "Browse" }).click();
  await settle(page, 600);
});
