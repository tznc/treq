// One dialog at a time: with the Stack dialog open, Ctrl+I opens no agent
// prompt over it, and Escape then closes the Stack dialog.
import { expect, test } from "@playwright/test";
import { capture } from "./capture.ts";
import { mod, openWorkspace, settle } from "./helpers.ts";

test("stacked dialogs: Ctrl+I over the Stack dialog, then Escape", async ({ page }) => {
  await openWorkspace(page, "feat/keyvalues-cache");
  const stack = page.getByRole("heading", { name: "Stack a new Workspace" });
  const prompt = page.getByRole("dialog", { name: "Start a new agent session" });
  await page.keyboard.press(`${mod}+Shift+N`);
  await expect(stack).toBeVisible();
  await settle(page, 800);
  await page.keyboard.press(`${mod}+i`);
  await settle(page, 1000);
  await capture(page, {
    name: "15-stacked-dialogs-01-ctrl-i",
    title: "Ctrl+I with the Stack dialog open",
    screen: true,
    rest: false,
    expectations: ["Only the Stack dialog shows; no agent prompt over it"],
  });
  await page.keyboard.press("Escape");
  await settle(page, 600);
  await page.keyboard.press("Escape");
  await settle(page, 800);
  await capture(page, {
    name: "15-stacked-dialogs-02-escape",
    title: "Then Escape twice",
    screen: true,
    rest: false,
    expectations: ["No dialog is left open; the workspace shows"],
  });
  await expect(prompt).toHaveCount(0);
  await expect(stack).toHaveCount(0);
});
