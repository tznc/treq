// Window limits: the workspace header at the largest UI zoom, and the
// window's minimum size.
import { expect, test } from "@playwright/test";
import { capture } from "./capture.ts";
import { mod, openTab, openWorkspace, row, settle } from "./helpers.ts";

const BRANCH = "feat/empty-event-message";

test.describe.configure({ mode: "serial" });

test("window-zoom: header at 200%", async ({ page }) => {
  await openWorkspace(page, BRANCH);
  await openTab(page, "Code");
  try {
    // 100% to 200% in 5% steps.
    for (let i = 0; i < 20; i++) await page.keyboard.press(`${mod}+=`);
    await settle(page, 1500);
    await expect(page.getByRole("button", { name: "Workspace details" })).toBeVisible();
    await capture(page, {
      name: "15-window-zoom-200",
      title: "Workspace header at 200% zoom",
      expectations: [
        "The header's buttons wrap onto further rows instead of running past the right edge",
        "Details and the overflow button are fully inside the window",
      ],
    });
  } finally {
    for (let i = 0; i < 20; i++) await page.keyboard.press(`${mod}+-`);
    await settle(page, 500);
  }
});

test("window-zoom: minimum window size", async ({ page }) => {
  await openWorkspace(page, BRANCH);
  try {
    await page.setViewportSize({ width: 400, height: 300 });
    await settle(page, 1500);
    await capture(page, {
      name: "15-window-min-size",
      title: "Window asked to shrink to 400x300",
      expectations: [
        "The window stops at its minimum size, wider and taller than 400x300",
        "Sidebar, header and tabs stay usable; nothing is cut in half",
      ],
    });
  } finally {
    await page.setViewportSize({ width: 1400, height: 900 });
    await settle(page, 500);
  }
});

test("window-zoom: terminal text follows the UI zoom", async ({ page }) => {
  await openWorkspace(page, BRANCH);
  await openTab(page, "Commits");
  await row(page, BRANCH).hover();
  await row(page, BRANCH).getByRole("button", { name: "Open shell" }).click();
  await settle(page, 2500);
  await page.mouse.click(1000, 780);
  await page.keyboard.type("PS1='$ '; clear; echo zoom-check-0123456789\n");
  await settle(page, 1000);
  // Zoom changes after the shell opened: the bug kept its open-time size.
  const steps: [number, number][] = [[80, -4], [100, 4], [150, 10]];
  try {
    for (const [zoom, by] of steps) {
      const key = by < 0 ? `${mod}+-` : `${mod}+=`;
      for (let i = 0; i < Math.abs(by); i++) await page.keyboard.press(key);
      await settle(page, 1500);
      await capture(page, {
        name: `15-window-zoom-terminal-${zoom}`,
        title: `Terminal at ${zoom}% zoom`,
        expectations: [
          `The terminal's text is scaled like the rest of the UI at ${zoom}%`,
          "The echoed line and prompt sit on whole cells, with no overlap or clipping",
        ],
      });
    }
  } finally {
    for (let i = 0; i < 10; i++) await page.keyboard.press(`${mod}+-`);
    await settle(page, 500);
  }
});
