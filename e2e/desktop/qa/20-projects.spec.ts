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
      "After: a project button ('Unsaved') sits left of the command palette in the top row",
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
  // Clear the prefilled name, then type the new one.
  await page.keyboard.press("End");
  for (let i = 0; i < 40; i++) await page.keyboard.press("Backspace");
  await page.keyboard.type("Event platform");
  await page.keyboard.press("Enter");
  // Past the "Saved project" toast.
  await settle(page, 6000);
  await expect(page.getByRole("button", { name: "Project: Event platform" })).toBeVisible();
  await capture(page, {
    name: "20-projects-04-saved",
    title: "Sidebar after saving the project",
    expectations: ["The project button reads 'Event platform' with a filled 'E' badge"],
  });

  await page.getByRole("button", { name: "Project: Event platform" }).click();
  await settle(page, 1000);
  await capture(page, {
    name: "20-projects-05-menu-saved",
    title: "Project menu of a saved project",
    screen: true,
    rest: false,
    expectations: [
      "Lists '✓ Event platform', then Rename Project…, Project Instructions… and New Window…",
    ],
  });
  const menu2 = popupBox();
  if (!menu2) throw new Error("no project menu");
  // Rows: the project, a separator, Rename, Instructions, New Window.
  await xPointer(page, menu2.x + 60, menu2.y + menu2.h - 41, 1);
  await settle(page, 1200);
  await page.keyboard.type("Use pnpm. Keep the API and the docs in sync.");
  await capture(page, {
    name: "20-projects-06-instructions",
    title: "Project instructions dialog",
    expectations: [
      "Dialog 'Event platform instructions' with an Instructions box (typed text) and a Context files box",
    ],
  });
  await page.getByRole("button", { name: "Save", exact: true }).click();
  await settle(page, 5000);

  await page.getByRole("button", { name: "Home", exact: true }).click();
  await settle(page, 1500);
  await expect(page.getByRole("heading", { name: "Event platform" })).toBeVisible();
  await capture(page, {
    name: "20-projects-07-project-home",
    title: "Project home",
    expectations: [
      "Heading 'Event platform' with the repository and workspace counts",
      "Two cards: lorem-ipsum/event-bus with its workspaces, docs-site with its workspace",
      "The Project home row is selected in the sidebar",
    ],
  });
  await page.getByRole("button", { name: "Open docs-site" }).click();
  await settle(page, 1500);
  await capture(page, {
    name: "20-projects-08-card-opened",
    title: "A card opens its repository",
    expectations: ["docs-site's home shows; its header row is selected in the sidebar"],
  });
  // The welcome page's Projects list is covered by the headless screen
  // tests (onboarding_lists_projects_and_opens_one): a second window does
  // not paint under Xvfb here.
});
