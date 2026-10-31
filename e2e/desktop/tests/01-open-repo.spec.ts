// The app opens the fixture repository: the sidebar's home row, the
// workspace tabs and the README on the Code tab (was Dashboard's
// open-repo flow in the React integration tests).
import { expect, test } from "@playwright/test";
import { appReady, homeRow } from "../fixture.ts";

test("opens the repository on its home view", async ({ page }) => {
  await appReady(page);
  await expect(page).toHaveTitle("Treq - Stacking ADE");
  await expect(homeRow(page)).toBeVisible();
  for (const tab of ["Code", "Commits", "Changes"]) {
    await expect(page.getByRole("tab", { name: tab })).toBeVisible();
  }
  await expect(
    page.getByRole("tab", { name: "Code", selected: true }),
  ).toBeVisible();
  // The README, rendered as Markdown, once: the text of earlier frames does
  // not linger after a few repaints (hovers; the e2e caret does not blink).
  await expect(page.getByText("e2e fixture")).toBeVisible();
  for (const tab of ["Commits", "Changes", "Code"]) {
    await page.getByRole("tab", { name: tab }).hover();
  }
  await expect(page.getByText("e2e fixture")).toHaveCount(1);
  // The home task input.
  await expect(
    page.getByRole("textbox", { name: "Describe a task…" }),
  ).toBeVisible();
});

test("switches between the workspace tabs", async ({ page }) => {
  await appReady(page);
  await page.getByRole("tab", { name: "Commits" }).click();
  await expect(
    page.getByRole("tab", { name: "Commits", selected: true }),
  ).toBeVisible();
  await page.getByRole("tab", { name: "Changes" }).click();
  await expect(
    page.getByRole("tab", { name: "Changes", selected: true }),
  ).toBeVisible();
  await expect(page.getByRole("textbox", { name: "Message" })).toBeVisible();
  await page.getByRole("tab", { name: "Code" }).click();
  await expect(
    page.getByRole("tab", { name: "Code", selected: true }),
  ).toBeVisible();
});
