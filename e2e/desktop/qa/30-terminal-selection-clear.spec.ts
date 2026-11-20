// A selection in a terminal, even one of blank cells, is dropped when the
// user clicks outside the terminal: no highlight blocks are left behind.
import { expect, test } from "@playwright/test";
import { capture } from "./capture.ts";
import { openTab, openWorkspace, row, settle } from "./helpers.ts";

const BRANCH = "feat/keyvalues-cache";

test("terminal-selection: clicking outside clears a blank selection", async ({
  page,
}) => {
  await openWorkspace(page, BRANCH);
  await openTab(page, "Commits");
  await row(page, BRANCH).hover();
  await row(page, BRANCH).getByRole("button", { name: "Open shell" }).click();
  await settle(page, 2500);
  await page.getByRole("button", { name: "Maximize" }).first().click();
  await settle(page, 1000);
  await page.mouse.click(700, 450);
  await page.keyboard.type("PS1='sel $ '; clear\n");
  await settle(page, 800);

  // Drag across blank rows below the prompt.
  await page.mouse.move(500, 300);
  await page.mouse.down();
  await page.mouse.move(900, 500, { steps: 10 });
  await page.mouse.up();
  await settle(page, 300);
  await capture(page, {
    name: "30-terminal-selection-01-dragged",
    title: "Blank cells selected in the shell",
    expectations: ["A highlighted block covers blank rows below the `sel $` prompt"],
  });

  // Click outside the terminal (the sidebar's empty space).
  await page.mouse.click(150, 800);
  await settle(page, 400);
  await capture(page, {
    name: "30-terminal-selection-02-clicked-away",
    title: "After clicking outside the terminal",
    expectations: [
      "No highlighted block remains anywhere in the terminal",
      "The `sel $` prompt is still shown with an outlined (unfocused) cursor",
    ],
  });
  await expect(page.getByRole("button", { name: "Restore" }).first()).toBeVisible();
});
