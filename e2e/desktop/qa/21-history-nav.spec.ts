// Back and forward: the title bar's arrows and the mouse's side buttons
// walk the places the window showed.
import { execFileSync } from "node:child_process";
import { expect, test, type Page } from "@playwright/test";
import { capture } from "./capture.ts";
import { home, settle } from "./helpers.ts";

test.describe.configure({ mode: "serial" });

const back = (page: Page) => page.getByRole("button", { name: "Previous view", exact: true });
const forward = (page: Page) => page.getByRole("button", { name: "Next view", exact: true });
const github = (page: Page) => page.getByRole("button", { name: "GitHub", exact: true });

/** X button 8 is the mouse's back button, 9 its forward button. */
function mouseSide(button: 8 | 9): void {
  execFileSync("xdotool", ["click", String(button)]);
}

test("history-nav: title bar arrows", async ({ page }) => {
  await home(page);
  await github(page).click();
  await expect(page.getByRole("button", { name: "GitHub", selected: true })).toBeVisible();
  await settle(page);
  await expect(back(page)).toBeEnabled();
  await expect(forward(page)).toBeDisabled();
  await capture(page, {
    name: "21-history-nav-01-on-github",
    title: "GitHub opened from the repository's home",
    expectations: [
      "Back and forward arrows sit at the right end of the sidebar's top strip",
      "Back is enabled; forward is greyed out",
      "The GitHub row is selected and the GitHub view shows",
    ],
  });

  await back(page).click();
  await expect(page.getByRole("tab", { name: "Code" })).toBeVisible();
  await settle(page);
  await expect(forward(page)).toBeEnabled();
  await capture(page, {
    name: "21-history-nav-02-back-to-home",
    title: "After pressing the back arrow",
    expectations: [
      "The repository's home view shows again, on its Code tab",
      "Forward is now enabled",
    ],
  });

  await forward(page).click();
  await expect(page.getByRole("button", { name: "GitHub", selected: true })).toBeVisible();
});

test("history-nav: mouse side buttons", async ({ page }) => {
  await home(page);
  await github(page).click();
  await expect(page.getByRole("button", { name: "GitHub", selected: true })).toBeVisible();
  mouseSide(8);
  await expect(page.getByRole("tab", { name: "Code" })).toBeVisible();
  mouseSide(9);
  await expect(page.getByRole("button", { name: "GitHub", selected: true })).toBeVisible();
  await settle(page);
  await capture(page, {
    name: "21-history-nav-03-mouse-forward",
    title: "Mouse back, then mouse forward",
    expectations: ["The GitHub view shows again after the mouse's forward button"],
  });
});
