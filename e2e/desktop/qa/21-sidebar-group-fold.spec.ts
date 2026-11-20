// Sidebar repo groups fold with a chevron after the repository's name.
import { expect, test } from "@playwright/test";
import { capture } from "./capture.ts";
import { home, ready, settle } from "./helpers.ts";

const clip = { x: 0, y: 0, width: 300, height: 420 };

test("sidebar group fold: trailing chevron", async ({ page }) => {
  await ready(page);
  await settle(page, 2500);
  await home(page);
  await capture(page, {
    name: "21-sidebar-group-fold-01-open",
    title: "Repo groups, open",
    expectations: ["A down chevron follows each repository name, inside its header row"],
    clip,
  });
  const fold = page.getByRole("button", { name: "Collapse docs-site" });
  await fold.click();
  await expect(page.getByRole("button", { name: "Expand docs-site" })).toBeVisible();
  await settle(page, 800);
  await capture(page, {
    name: "21-sidebar-group-fold-02-shut",
    title: "docs-site folded",
    expectations: ["docs-site shows a right chevron after its name", "event-bus stays open"],
    clip,
  });

  // Drag the sidebar narrow so the repository names run out of room.
  const settings = await page.getByRole("button", { name: "Settings", exact: true }).boundingBox();
  if (!settings) throw new Error("sidebar not drawn");
  const edge = settings.x + settings.width + 6;
  await page.mouse.move(edge, 300);
  await page.mouse.down();
  await page.mouse.move(150, 300, { steps: 8 });
  await page.mouse.up();
  await settle(page, 800);
  await capture(page, {
    name: "21-sidebar-group-fold-03-narrow",
    title: "Long repository names in a narrow sidebar",
    expectations: [
      "Each repository name ends in an ellipsis",
      "The chevron after each name is whole and inside its header row",
    ],
    clip,
  });
});
