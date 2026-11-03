// The delete-workspace confirmation is drawn in the repo window itself, over
// everything in it: the title asks the question, and the buttons are Cancel
// and "Delete workspace". Escape cancels, so nothing is deleted.
import { expect, test, type Page } from "@playwright/test";
import { capture } from "./capture.ts";
import { home, mod, settle } from "./helpers.ts";

test.describe.configure({ mode: "serial" });

const BRANCH = "feat/keyvalues-cache";

/** Picks Delete Workspace for `BRANCH` from the palette. */
async function askDelete(page: Page): Promise<void> {
  await page.keyboard.press(`${mod}+k`);
  await page.getByRole("textbox", { name: "Type a command or search…" }).fill("Delete workspace");
  await page.getByRole("option", { name: "Delete workspace" }).click();
  const picker = page.getByRole("textbox", { name: "Search workspaces to delete…" });
  await expect(picker).toBeVisible();
  await picker.fill(BRANCH);
  await settle(page, 600);
  await page.keyboard.press("Enter");
}

test("delete-confirmation: one in-app question; Escape cancels", async ({ page }) => {
  await home(page);
  await askDelete(page);
  const question = page.getByRole("dialog", { name: `Delete ${BRANCH}?`, exact: true });
  await expect(question).toBeVisible();
  await expect(question).toHaveCount(1);
  await expect(page.getByTestId("confirm-cancel")).toHaveAccessibleName("Cancel");
  await expect(page.getByTestId("confirm-confirm")).toHaveAccessibleName("Delete workspace");
  await settle(page, 600);
  await capture(page, {
    name: "15-delete-confirmation-asked",
    title: "Delete Workspace: the in-app question",
    screen: true,
    rest: false,
    expectations: [
      `The title asks 'Delete ${BRANCH}?' over a scrim that covers the whole window`,
      "Cancel and a red 'Delete workspace' button; no Yes/No",
    ],
  });
  await page.keyboard.press("Escape");
  await expect(question).toHaveCount(0);
  await expect(page.getByRole("button", { name: new RegExp(BRANCH) }).first()).toBeVisible();
});
