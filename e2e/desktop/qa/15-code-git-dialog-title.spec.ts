// The Code tab's file list leaves out the .git and .jj directories, a
// hovered workspace row keeps its branch name clear of the row actions, and
// the window the palette and other dialogs open in is titled "Treq".
import { expect, test } from "@playwright/test";
import { capture } from "./capture.ts";
import { home, mod, overlay, row, settle } from "./helpers.ts";

test.describe.configure({ mode: "serial" });

const BRANCH = "feat/empty-event-message";

test("code-git: the Code tab hides version control directories", async ({ page }) => {
  await home(page);
  await settle(page, 2000);
  await capture(page, {
    name: "15-code-git-hidden",
    title: "Home Code tab's file list",
    expectations: ["The file list shows packages, .gitignore and README.md, with no .git or .jj row"],
  });
  const entry = (name: string) => page.getByRole("button", { name, exact: true });
  await expect(entry("README.md")).toBeVisible();
  await expect(entry(".git")).toHaveCount(0);
  await expect(entry(".jj")).toHaveCount(0);
});

test("row-hover: a hovered workspace row keeps its branch name clear of the actions", async ({ page }) => {
  await home(page);
  await row(page, BRANCH).hover();
  await settle(page, 1500);
  await expect(row(page, BRANCH).getByRole("button", { name: "Stack on this…" })).toBeVisible();
  await capture(page, {
    name: "15-row-hover-actions",
    title: "Hovered workspace row at the default sidebar width",
    screen: true,
    rest: false,
    expectations: ["The feat/empty-event-message title ends in an ellipsis before its three action buttons"],
  });
});

test("dialog-title: the palette window is titled Treq", async ({ page, context }) => {
  await home(page);
  await page.keyboard.press(`${mod}+k`);
  const dialog = await overlay(page, context);
  await expect(dialog.getByRole("textbox").first()).toBeVisible();
  expect(await dialog.title()).toBe("Treq");
  await dialog.keyboard.press("Escape");
});
