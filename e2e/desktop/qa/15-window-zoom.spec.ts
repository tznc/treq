// Window limits: the workspace header at the largest UI zoom, and the
// window's minimum size.
import { expect, test } from "@playwright/test";
import { capture } from "./capture.ts";
import { mod, openTab, openWorkspace, settle } from "./helpers.ts";

const BRANCH = "feat/empty-event-message";

test.describe.configure({ mode: "serial" });

test("window-zoom: header at 200%", async ({ page }) => {
  await openWorkspace(page, BRANCH);
  await openTab(page, "Code");
  try {
    // 100% to 200% in 5% steps.
    for (let i = 0; i < 20; i++) await page.keyboard.press(`${mod}+=`);
    await settle(page, 1500);
    await expect(page.getByRole("button", { name: "Details" })).toBeVisible();
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
