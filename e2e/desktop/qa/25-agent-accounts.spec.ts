// Settings: agent account profiles on the Application tab, added through the
// form and listed with each agent's default account.
import { expect, test } from "@playwright/test";
import { capture } from "./capture.ts";
import { ready, settle } from "./helpers.ts";

test("agent accounts: add profiles", async ({ page }) => {
  await ready(page);
  await page.getByRole("button", { name: "Settings", exact: true }).click();
  await page.getByRole("tab", { name: "Application" }).click();
  await settle(page, 1200);

  const name = page.getByRole("textbox", { name: "work" });
  await name.scrollIntoViewIfNeeded();
  for (const profile of ["work", "personal"]) {
    await name.fill(profile);
    await settle(page, 300);
    await page.getByRole("button", { name: "Add", exact: true }).click();
    await settle(page, 600);
  }
  await expect(page.getByRole("button", { name: "Remove", exact: true })).toHaveCount(2);

  // A name with spaces is refused in place.
  await name.fill("My Work");
  await page.getByRole("button", { name: "Add", exact: true }).click();
  await settle(page, 600);
  await expect(page.getByRole("button", { name: "Remove", exact: true })).toHaveCount(2);

  await capture(page, {
    name: "25-agent-accounts-01-profiles",
    title: "Settings: Agent accounts",
    expectations: [
      "An 'Agent accounts' section lists 'Claude · work' and 'Claude · personal', each with a Remove button",
      "'Default Claude account' and 'Default Codex account' selects sit above the list",
      "Below the Add row, a red error explains the allowed name characters",
    ],
  });
});

test("agent accounts: profile modes", async ({ page }) => {
  await ready(page);
  if ((await page.getByRole("button", { name: "Back", exact: true }).count()) === 0) {
    await page.getByRole("button", { name: "Settings", exact: true }).click();
  }
  await page.getByRole("tab", { name: "Application" }).click();
  await settle(page, 1200);
  const profile = page.getByRole("textbox", { name: "work" });
  await profile.scrollIntoViewIfNeeded();
  if ((await page.getByRole("button", { name: "Remove", exact: true }).count()) === 0) {
    await profile.fill("work");
    await page.getByRole("button", { name: "Add", exact: true }).click();
    await settle(page, 800);
  }

  const name = page.getByRole("textbox", { name: "deep" });
  await name.scrollIntoViewIfNeeded();
  await name.fill("deep");
  await page.getByRole("textbox", { name: "Model, such as opus" }).fill("opus");
  for (const [combobox, downs] of [["Effort", 3], ["Permission mode", 2]] as const) {
    await page.getByRole("combobox", { name: combobox, exact: true }).click();
    for (let i = 0; i < downs; i++) await page.keyboard.press("ArrowDown");
    await page.keyboard.press("Enter");
    await settle(page, 300);
  }
  await page
    .getByRole("textbox", { name: "Added to the agent's system prompt in this mode" })
    .fill("Think it through before editing.");
  await page.getByRole("button", { name: "Save mode", exact: true }).click();
  await settle(page, 900);
  await expect(page.getByRole("button", { name: "Edit", exact: true })).toHaveCount(1);
  await page.getByRole("button", { name: "Save mode", exact: true }).scrollIntoViewIfNeeded();
  await settle(page, 500);

  await capture(page, {
    name: "25-agent-accounts-02-modes",
    title: "Settings: a profile mode",
    expectations: [
      "Under 'Modes', a row 'Claude · work · deep' summarizes opus, high effort and Plan, with Edit and Remove",
      "A 'Default mode for Claude · work' select sits above the modes",
      "The mode form is empty again: profile, name, model, effort, permission and instructions, then Save mode",
    ],
  });
});
