// settings.test.tsx and agent-notification-setting.test.tsx against the
// real backend: the settings page opens from the sidebar, saves to the app
// database, and shows the saved value when it opens again.
import { expect, test, type Page } from "@playwright/test";
import { appReady, goHome } from "../fixture.ts";

async function openApplicationSettings(page: Page) {
  await page.getByRole("button", { name: "Settings", exact: true }).click();
  await page.getByRole("tab", { name: "Application" }).click();
  await expect(
    page.getByRole("tab", { name: "Application", selected: true }),
  ).toBeVisible();
  return page.getByRole("switch", { name: "Notify when an agent finishes" });
}

test("opens on the repository tab and closes back to the repository", async ({
  page,
}) => {
  await appReady(page);
  await page.getByRole("button", { name: "Settings", exact: true }).click();
  for (const tab of [
    "Repository",
    "Application",
    "Account",
    "Integrations",
    "Feature preview",
  ]) {
    await expect(page.getByRole("tab", { name: tab })).toBeVisible();
  }
  await expect(
    page.getByRole("switch", { name: "Auto-push to remote" }),
  ).toBeVisible();
  await page.getByRole("button", { name: "Close" }).click();
  await expect(page.getByRole("tab", { name: "Code" })).toBeVisible();
});

test("saves the agent notification switch and shows it when settings reopen", async ({
  page,
}) => {
  await goHome(page);
  const toggle = await openApplicationSettings(page);
  // On by default.
  await expect(toggle).toBeChecked();
  await toggle.click();
  await expect(toggle).not.toBeChecked();
  await page.getByRole("button", { name: "Save settings" }).click();
  await page.getByRole("button", { name: "Close" }).click();
  await expect(page.getByRole("tab", { name: "Code" })).toBeVisible();

  const reopened = await openApplicationSettings(page);
  await expect(reopened).not.toBeChecked();
  // Back on for the other tests.
  await reopened.click();
  await page.getByRole("button", { name: "Save settings" }).click();
  await page.getByRole("button", { name: "Close" }).click();
});
