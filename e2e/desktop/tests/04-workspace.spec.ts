// The New workspace dialog against the real backend: Stack opens it,
// and creating a workspace for a new branch adds a jj workspace on disk and
// a row to the sidebar.
import { existsSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { expect, test } from "@playwright/test";
import { fixture, goHome } from "../fixture.ts";

const heading = { name: "New workspace" };

test("creates a workspace from the New workspace dialog", async ({ page }) => {
  await goHome(page);
  await page.getByRole("button", { name: "New workspace…" }).first().click();
  await expect(page.getByRole("heading", heading)).toBeVisible();

  const lines = page.getByLabel("Workspaces", { exact: true });
  await lines.fill("e2e/bad..");
  await expect(page.getByRole("alert", { name: "1 line can't be opened" })).toBeVisible({
    timeout: 20_000,
  });
  await expect(page.getByRole("button", { name: "Create workspace" })).toBeDisabled();
  await lines.fill("e2e/first");
  const create = page.getByRole("button", { name: "Create workspace" });
  await expect(create).toBeEnabled({ timeout: 20_000 });
  await create.click();

  await expect(page.getByRole("heading", heading)).toHaveCount(0, { timeout: 30_000 });
  const row = page.getByRole("button", { name: "e2e/first", exact: true });
  await expect(row).toBeVisible();

  const workspaces = join(fixture().repo, ".treq", "workspaces");
  await expect
    .poll(() => existsSync(workspaces) && readdirSync(workspaces).length)
    .toBe(1);

  // The new workspace opens on its own view.
  await row.click();
  await expect(
    page.getByRole("button", {
      name: "e2e/first",
      exact: true,
      selected: true,
    }),
  ).toBeVisible();
});

test("Cancel closes the dialog without creating anything", async ({ page }) => {
  await goHome(page);
  await page.getByRole("button", { name: "New workspace…" }).first().click();
  await page.getByLabel("Workspaces", { exact: true }).fill("e2e/never");
  await page.getByRole("button", { name: "Cancel" }).click();
  await expect(page.getByRole("heading", heading)).toHaveCount(0);
  await expect(
    page.getByRole("button", { name: "e2e/never", exact: true }),
  ).toHaveCount(0);
});
