// The delete-workspace confirmation is drawn in the repo window itself, over
// everything in it: the title asks the question, and the buttons are Cancel
// and "Delete workspace". Escape cancels, so nothing is deleted.
import { expect, test, type BrowserContext, type Page } from "@playwright/test";
import { capture } from "./capture.ts";
import { home, mod, overlay, settle } from "./helpers.ts";

test.describe.configure({ mode: "serial" });

const BRANCH = "feat/keyvalues-cache";

/** Picks Delete Workspace for `BRANCH` from the palette. */
async function askDelete(page: Page, context: BrowserContext): Promise<void> {
  await page.keyboard.press(`${mod}+k`);
  const palette = await overlay(page, context);
  await palette.getByRole("textbox", { name: "Type a command or search…" }).fill("Delete workspace");
  await palette.getByRole("option", { name: "Delete workspace" }).click();
  // The picker draws in the repo window or in the shared dialog window.
  const find = (p: Page) => p.getByRole("textbox", { name: "Search workspaces to delete…" });
  let picker: Page | undefined;
  await expect
    .poll(async () => {
      picker = (await find(page).isVisible())
        ? page
        : (await find(palette).isVisible())
          ? palette
          : undefined;
      return !!picker;
    })
    .toBe(true);
  if (!picker) throw new Error("no delete picker");
  await find(picker).fill(BRANCH);
  await settle(page, 600);
  await picker.keyboard.press("Enter").catch(() => {});
}

test("delete-confirmation: one in-app question; Escape cancels", async ({ page, context }) => {
  await home(page);
  await askDelete(page, context);
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
