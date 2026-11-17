// Settings: every tab (Repository, Application, Account, Integrations,
// Skills, Feature Preview), the long tabs scrolled, the theme select
// switching the app live to light and back, the Linear API key dialog,
// a skills search and a field saving itself.
//
// A select's options are not observable: they are clicked at their place
// under the select (one row is 30px).
import { expect, test, type Page } from "@playwright/test";
import { capture, theme } from "./capture.ts";
import { ready, settle } from "./helpers.ts";

test.describe.configure({ mode: "serial" });

async function settings(page: Page, tab: string): Promise<void> {
  await ready(page);
  const close = page.getByRole("button", { name: "Back", exact: true });
  if ((await close.count()) === 0) {
    await page.getByRole("button", { name: "Settings", exact: true }).click();
    await expect(close).toBeVisible();
  }
  await page.getByRole("tab", { name: tab }).click();
  await expect(page.getByRole("tab", { name: tab, selected: true })).toBeVisible();
  // Park the pointer off the sidebar so the Settings tooltip is gone.
  await page.mouse.move(1000, 860);
  await settle(page, 1200);
}

async function scroll(page: Page, dy: number): Promise<void> {
  await page.mouse.move(950, 500);
  await page.mouse.wheel(0, dy);
  await settle(page, 900);
}

test("settings: repository", async ({ page }) => {
  await settings(page, "Repository");
  await capture(page, {
    name: "11-settings-repository",
    title: "Settings: Repository tab",
    expectations: [
      "Nav on the left, form column centred; labels, fields and help text aligned",
      "A back arrow left of the Settings title; no Save or Close buttons",
    ],
  });
  await scroll(page, 900);
  await capture(page, {
    name: "11-settings-repository-scrolled",
    title: "Settings: Repository tab, lower sections",
    expectations: ["Code Review and later sections: selects and switches aligned to the column"],
  });
  await scroll(page, -2000);
  const pattern = page.getByRole("textbox", { name: "treq/{name}" });
  await pattern.fill("qa/{name}");
  // The field saves once typing pauses (500ms), then shows its tick for
  // 1.5s (a screen test checks it; the tick itself is not observable).
  await settle(page, 900);
  await capture(page, {
    name: "11-settings-saved-tick",
    title: "Settings: a typed field saved itself",
    expectations: [
      "The preview under Branch name pattern follows the edit (qa/add-dark-mode)",
      "A muted check and 'Saved' sit right of the Branch name pattern label",
    ],
  });
  await pattern.fill("treq/{name}");
  await settle(page, 2500);
});

test("settings: application and live theme switch", async ({ page }) => {
  await settings(page, "Application");
  await capture(page, {
    name: "11-settings-application",
    title: "Settings: Application tab",
    expectations: [
      "Theme, zoom slider, font size, model and agent selects in one column",
      "Command-line tools list: installed marks vs Install links aligned right",
    ],
  });
  const select = page.getByRole("combobox").first();
  const box = await select.boundingBox();
  if (!box) throw new Error("no theme select");
  await select.click();
  await settle(page, 800);
  await capture(page, {
    name: "11-settings-theme-select",
    title: "Settings: Theme select open",
    expectations: ["System / Light / Dark listed under the select; current one checked"],
  });
  const option = (row: number) => page.mouse.click(box.x + 40, box.y + box.height + 26 + row * 30);
  const other = theme === "dark" ? 1 : 2; // Light or Dark
  await option(other);
  await settle(page, 1500);
  await capture(page, {
    name: "11-settings-theme-live-switched",
    title: "Settings: theme switched live (not saved)",
    expectations: [
      "The whole window, sidebar included, repaints in the other theme",
      "No element keeps the old theme's colours",
    ],
  });
  await select.click();
  await settle(page, 600);
  await option(theme === "dark" ? 2 : 1);
  await settle(page, 1200);
  // The select's value is its accessible value, not its text.
  await expect
    .poll(() => select.ariaSnapshot())
    .toContain(theme === "dark" ? '"Dark"' : '"Light"');

  await scroll(page, 900);
  await capture(page, {
    name: "11-settings-application-scrolled",
    title: "Settings: Application tab, lower sections",
    expectations: [
      "Merge tool select, notifications switch, setup script field and Run aligned",
    ],
  });
  await scroll(page, 2000);
  await capture(page, {
    name: "11-settings-application-bottom",
    title: "Settings: Application tab, end of the page",
    expectations: ["The last section ends with padding, nothing cut off"],
  });
});

test("settings: account, integrations, skills, feature preview", async ({ page }) => {
  await settings(page, "Account");
  // The pasted-callback box is for `cargo run` builds only.
  await expect(page.getByRole("textbox", { name: "treq://auth/callback?token=…" })).toHaveCount(0);
  await capture(page, {
    name: "11-settings-account",
    title: "Settings: Account tab (signed out)",
    expectations: [
      "Sign in with Browser, and no Dev paste-a-callback panel",
      "Copy explains what an account adds",
    ],
  });
  await settings(page, "Integrations");
  await capture(page, {
    name: "11-settings-integrations",
    title: "Settings: Integrations tab",
    expectations: [
      "One collapsed card each for GitHub and Linear: icon, name, a one-line purpose",
      "Each card shows 'Not connected' or 'Connected' with a dot, and a chevron on the right",
    ],
  });
  await page.getByRole("button", { name: "GitHub integration" }).click();
  await settle(page, 600);
  await capture(page, {
    name: "11-settings-integrations-github-signed-out",
    title: "Settings: GitHub card expanded while signed out",
    expectations: [
      "The GitHub card is open: 'Sign in to Treq Cloud to connect GitHub' and a Sign in with browser button",
      "No full-page sign-in block",
    ],
  });
  await page.getByRole("button", { name: "Linear integration" }).click();
  await settle(page, 600);
  await capture(page, {
    name: "11-settings-integrations-linear",
    title: "Settings: Linear card expanded",
    expectations: [
      "The Linear card is open: an 'API key' row reading 'Paste a Linear personal API key' with Add API key",
      "A 'Create a key in Linear' link under it",
    ],
  });
  await page.getByRole("button", { name: "Add API key" }).click();
  await settle(page, 1000);
  await capture(page, {
    name: "11-settings-linear-key",
    title: "Settings: Linear API key entry",
    screen: true,
    rest: false,
    expectations: ["Key field (masked) with Save / Cancel; link to where to get a key"],
  });
  await page.keyboard.press("Escape");
  const cancel = page.getByRole("button", { name: "Cancel" });
  if ((await cancel.count()) > 0) await cancel.first().click();
  await settle(page, 600);

  await settings(page, "Skills");
  await capture(page, {
    name: "11-settings-skills",
    title: "Settings: Skills tab",
    expectations: [
      "Search and source filter on top; skills listed with names, descriptions, Install…",
      "Long descriptions truncate or wrap without pushing Install off the row",
    ],
  });
  await page.getByRole("textbox", { name: "Search by name or description" }).fill("react");
  await settle(page, 1200);
  await capture(page, {
    name: "11-settings-skills-search",
    title: "Settings: Skills filtered by 'react'",
    expectations: ["Only matching skills, or an empty state"],
  });
  await page.getByRole("textbox", { name: "Search by name or description" }).fill("zzzz-no-skill");
  await settle(page, 1200);
  await capture(page, {
    name: "11-settings-skills-empty",
    title: "Settings: Skills search with no match",
    expectations: ["An explicit no-results state"],
  });
  await page.getByRole("textbox", { name: "Search by name or description" }).fill("");

  await settings(page, "Feature preview");
  await expect(page.getByRole("button", { name: "Logs docs" })).toBeVisible();
  await capture(page, {
    name: "11-settings-feature-preview",
    title: "Settings: Feature Preview tab",
    expectations: [
      "Feature names in plain text, not link-coloured",
      "Each row has a Docs button and a switch on the right",
    ],
  });
  await page.getByRole("button", { name: "Back", exact: true }).click();
  await settle(page, 800);
});
