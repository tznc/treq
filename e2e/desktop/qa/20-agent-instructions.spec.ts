// Settings: the Agent instructions fields on the Application and Repository
// tabs, filled and saved.
import { expect, test, type Page } from "@playwright/test";
import { capture } from "./capture.ts";
import { ready, settle } from "./helpers.ts";

test.describe.configure({ mode: "serial" });

async function settings(page: Page, tab: string): Promise<void> {
  await ready(page);
  const back = page.getByRole("button", { name: "Back", exact: true });
  if ((await back.count()) === 0) {
    await page.getByRole("button", { name: "Settings", exact: true }).click();
    await expect(back).toBeVisible();
  }
  await page.getByRole("tab", { name: tab }).click();
  await expect(page.getByRole("tab", { name: tab, selected: true })).toBeVisible();
  await page.mouse.move(1000, 860);
  await settle(page, 1200);
}

async function fillInstructions(page: Page, placeholder: string, text: string): Promise<void> {
  const field = page.getByRole("textbox", { name: placeholder });
  await field.scrollIntoViewIfNeeded();
  await field.fill(text);
  await settle(page, 900);
}

test("agent instructions: application", async ({ page }) => {
  await settings(page, "Application");
  await page.mouse.move(950, 500);
  await page.mouse.wheel(0, 4000);
  await settle(page, 900);
  await fillInstructions(page, "Added to every agent's system prompt", "Be terse. Prefer TDD.");
  await capture(page, {
    name: "20-agent-instructions-application",
    title: "Settings: Application agent instructions",
    expectations: [
      "An 'Agent instructions' textarea sits last, below the setup script",
      "It shows 'Be terse. Prefer TDD.' with help text about every repository",
    ],
  });
});

test("agent instructions: repository", async ({ page }) => {
  await settings(page, "Repository");
  await fillInstructions(
    page,
    "Added to every agent's system prompt in this repository",
    "Use cargo nextest.",
  );
  await capture(page, {
    name: "20-agent-instructions-repository",
    title: "Settings: Repository agent instructions",
    expectations: [
      "An 'Agent instructions' textarea follows Default agent",
      "It shows 'Use cargo nextest.' with help text about application instructions",
    ],
  });
});
