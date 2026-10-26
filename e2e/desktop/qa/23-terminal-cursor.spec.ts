// The terminal cursor sits on the cell it belongs to at any UI zoom: at a
// shell prompt on the bottom row of a maximized pane (where a zoom error
// adds up most) and inside vim.
import { expect, type Page, test } from "@playwright/test";
import { capture } from "./capture.ts";
import { mod, openTab, openWorkspace, row, settle } from "./helpers.ts";

const BRANCH = "feat/keyvalues-cache";

test.describe.configure({ mode: "serial" });

// The focused bar blinks, so capture unfocused: the outlined cell is always
// drawn, at the same position. Clicking the sidebar's empty space blurs it.
async function typeThenCapture(
  page: Page,
  keys: string,
  name: string,
  title: string,
  expectations: string[],
) {
  await page.keyboard.type(keys);
  await page.mouse.click(150, 800);
  await settle(page, 400);
  await capture(page, { name, title, expectations });
  await page.mouse.click(700, 450);
}

async function zoom(page: Page, steps: number) {
  const key = steps > 0 ? `${mod}+=` : `${mod}+-`;
  for (let i = 0; i < Math.abs(steps); i++) await page.keyboard.press(key);
  await settle(page, 1500);
}

test("terminal-cursor: on its cell at 80%, 100% and 150% zoom", async ({
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
  await page.keyboard.type("PS1='cursor $ '; clear; seq 300\n");
  await settle(page, 1000);

  for (const [label, steps] of [
    ["080", -4],
    ["100", 0],
    ["150", 10],
  ] as const) {
    await zoom(page, steps);
    // Zoom changes the cell size, so the grid reflows: start a fresh prompt.
    await page.keyboard.type("\n");
    await settle(page, 500);
    await typeThenCapture(
      page,
      "echo here",
      `23-terminal-cursor-shell-${label}`,
      `Shell prompt on the bottom row at ${label}% zoom`,
      [
        "The prompt `cursor $ echo here` is on the pane's last text row",
        "The outlined cursor cell sits right after `echo here`, on the same row, not above or below it",
      ],
    );
    await page.keyboard.press("Control+U");
    await zoom(page, -steps);
  }

  // vim: the cursor on line 30, column 5 of a numbered buffer, at 80%.
  await zoom(page, -4);
  await page.keyboard.type("seq 100 | vim -R - +30 '+normal 0'\n");
  await settle(page, 2000);
  await typeThenCapture(
    page,
    "4l",
    "23-terminal-cursor-vim-080",
    "vim at 80% zoom, cursor on line 30",
    [
      "vim shows the numbers 1..100 with `30` partway down",
      "The outlined cursor cell is on the row reading `30`, not on a row above it",
    ],
  );
  await page.keyboard.type(":q!\n");
  await zoom(page, 4);
  await expect(
    page
      .getByRole("button", { name: "Maximize" })
      .or(page.getByRole("button", { name: "Restore" }))
      .first(),
  ).toBeVisible();
});
