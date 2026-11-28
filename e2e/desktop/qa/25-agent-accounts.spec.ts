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
