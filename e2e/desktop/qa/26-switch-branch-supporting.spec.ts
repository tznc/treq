// Switch Branch from the home-row menu of a repository that is not on
// screen: the menu offers it, and picking it opens that repository's
// branch switcher.
import { expect, test } from "@playwright/test";
import { capture } from "./capture.ts";
import { home, popupBox, ready, settle, xPointer } from "./helpers.ts";

test("switch-branch-supporting: unselected repo menu opens its switcher", async ({ page }) => {
  await ready(page);
  await home(page);
  // The docs-site home row has no accessible name: below event-bus's four workspaces.
  await page.mouse.click(90, 389, { button: "right" });
  await settle(page, 1000);
  await capture(page, {
    name: "26-01-unselected-repo-menu",
    title: "Home-row menu of the unselected docs-site repository",
    screen: true,
    rest: false,
    expectations: [
      "event-bus stays selected; the menu opens on the docs-site row",
      "Switch Branch... is the first item, above the path items",
    ],
  });

  const menu = popupBox();
  if (!menu) throw new Error("no menu");
  xPointer(menu.x + 40, menu.y + 18, 1);
  await settle(page, 1500);
  await expect(page.getByPlaceholder("Search branches…")).toBeVisible();
  await capture(page, {
    name: "26-02-switcher-for-docs-site",
    title: "Branch switcher opened for docs-site",
    expectations: [
      "The Switch Branch dialog is open with docs-site's branches",
      "The header and sidebar now show docs-site as the active repository",
    ],
  });
});
