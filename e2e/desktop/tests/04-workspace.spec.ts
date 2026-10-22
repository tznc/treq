// workspace/create-dialog-branch-name.test.tsx and stack-from-home against
// the real backend: Stack opens "Stack a new Workspace", and creating it
// adds a jj workspace on disk and a row to the sidebar.
import { existsSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { expect, test } from "@playwright/test";
import { fixture, goHome } from "../fixture.ts";

test("creates a workspace from the Stack dialog", async ({ page }) => {
  await goHome(page);
  await page.getByRole("button", { name: "Stack", exact: true }).click();
  await expect(
    page.getByRole("heading", { name: "Stack a new Workspace" }),
  ).toBeVisible();

  const branch = page.getByRole("textbox", { name: "Branch Name" });
  const create = page.getByRole("button", { name: "Create Workspace" });
  await branch.fill("bad name..");
  await expect(create).toBeDisabled();
  await branch.fill("e2e/first");
  await expect(create).toBeEnabled();
  await create.click();

  await expect(
    page.getByRole("heading", { name: "Stack a new Workspace" }),
  ).toHaveCount(0);
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
  await page.getByRole("button", { name: "Stack", exact: true }).click();
  await page.getByRole("textbox", { name: "Branch Name" }).fill("e2e/never");
  await page.getByRole("button", { name: "Cancel" }).click();
  await expect(
    page.getByRole("heading", { name: "Stack a new Workspace" }),
  ).toHaveCount(0);
  await expect(
    page.getByRole("button", { name: "e2e/never", exact: true }),
  ).toHaveCount(0);
});
