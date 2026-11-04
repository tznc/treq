// Projects: the window's repositories grouped under repo headers (each the
// repository's home row), the project button beside the command palette,
// its menu, the Save project dialog, and the welcome page's project list.
// On a build without projects the same steps capture the old sidebar, so
// one spec makes the before/after pair.
import { expect, test } from "@playwright/test";
import { capture } from "./capture.ts";
import { home, popupBox, ready, settle, xPointer } from "./helpers.ts";

test.describe.configure({ mode: "serial" });

test("projects: sidebar, project menu, save and welcome list", async ({ page }) => {
  await ready(page);
  await settle(page, 2500);
  await home(page);
  await capture(page, {
    name: "20-projects-01-sidebar",
    title: "Sidebar of a window with two repositories",
    expectations: [
      "After: a project button ('Unsaved project') sits left of the command palette in the top row",
      "After: each repository is a group header that is its home row (name + branch), workspaces stacked under it",
      "Before: home rows for both repositories above Github/Linear, plain group labels in the list",
    ],
  });

  const button = page.getByRole("button", { name: /^Project: / });
  if ((await button.count()) === 0) return; // the build before projects

  await button.click();
  await settle(page, 1000);
  const menu = popupBox();
  await capture(page, {
    name: "20-projects-02-menu-unsaved",
    title: "Project menu of an unsaved project",
    screen: true,
    rest: false,
    expectations: ["The menu offers Save Project… and New Window…"],
  });
  if (!menu) throw new Error("no project menu");
  // "Save Project…" is the first row.
  await xPointer(page, menu.x + 60, menu.y + 18, 1);
  await settle(page, 1000);
  await capture(page, {
    name: "20-projects-03-save-dialog",
    title: "Save project dialog",
    expectations: [
      "Dialog titled 'Save project' with the name prefilled from the repository folders ('event-bus + docs-site')",
      "Lists the repositories and offers Cancel and Save project",
    ],
  });
  await page.keyboard.press(process.platform === "darwin" ? "Meta+A" : "Control+A");
  await page.keyboard.type("Event platform");
  await page.keyboard.press("Enter");
  await settle(page, 2000);
  await expect(page.getByRole("button", { name: "Project: Event platform" })).toBeVisible();
  await capture(page, {
    name: "20-projects-04-saved",
    title: "Sidebar after saving the project",
    expectations: ["The project button reads 'Event platform' with a filled 'E' badge"],
  });

  await page.getByRole("button", { name: "Project: Event platform" }).click();
  await settle(page, 1000);
  const saved = popupBox();
  await capture(page, {
    name: "20-projects-05-menu-saved",
    title: "Project menu of a saved project",
    screen: true,
    rest: false,
    expectations: [
      "Lists '✓ Event platform', then Rename Project… and New Window…",
    ],
  });
  if (!saved) throw new Error("no project menu");
  // "New Window…" is the last of four rows (project, separator, rename, new).
  await xPointer(page, saved.x + 60, saved.y + saved.h - 18, 1);
  await settle(page, 6000);
  await capture(page, {
    name: "20-projects-06-welcome",
    title: "New window: welcome page with the Projects list",
    screen: true,
    rest: false,
    expectations: [
      "A new window shows the welcome page with a 'Projects' section listing 'Event platform' (event-bus, docs-site)",
      "Recent repositories follow below",
    ],
  });
});
