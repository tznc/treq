// Branch switcher: Enter on an "In workspaces" row switches the home
// checkout onto that branch, the current branch stays under "Main tree",
// and the workspace's Changes still lists its committed files.
import { expect, test } from "@playwright/test";
import { ALERTING } from "../landing/seed.ts";
import { capture } from "./capture.ts";
import { home, mod, openTab, settle } from "./helpers.ts";

test.describe.configure({ mode: "serial" });

async function switcher(page: import("@playwright/test").Page) {
  await page.keyboard.press(`${mod}+k`);
  await page.getByRole("textbox", { name: "Type a command or search…" }).fill("Switch branch");
  await page.getByRole("option", { name: "Switch branch" }).click();
  await expect(page.getByPlaceholder("Search branches…")).toBeVisible();
  await settle(page, 800);
}

test("switch-home-to-workspace-branch", async ({ page }) => {
  await home(page);
  await switcher(page);
  await capture(page, {
    name: "31-01-switcher-before",
    title: "Branch switcher on the home checkout (main)",
    expectations: [
      "main is under Main tree",
      `${ALERTING} is under In workspaces, labelled with its workspace`,
    ],
  });

  await page.getByPlaceholder("Search branches…").fill(ALERTING);
  await settle(page, 400);
  await page.keyboard.press("Enter");
  await expect(page.getByPlaceholder("Search branches…")).toHaveCount(0);
  await settle(page, 1500);
  await capture(page, {
    name: "31-02-home-switched",
    title: `Home after Enter on ${ALERTING}`,
    expectations: [
      "The switcher closed without opening the workspace",
      `The home row / header shows ${ALERTING} as the checked-out branch`,
    ],
  });

  await switcher(page);
  await capture(page, {
    name: "31-03-switcher-after",
    title: "Branch switcher after the switch",
    expectations: [`${ALERTING} is first, under Main tree`],
  });
  await page.keyboard.press("Escape");

  // The home row now shares the branch's name; pick the workspace row.
  await page.getByRole("button", { name: ALERTING, exact: true }).nth(1).click();
  await settle(page, 800);
  await openTab(page, "Changes");
  await settle(page, 1500);
  await capture(page, {
    name: "31-04-workspace-changes",
    title: `${ALERTING} Changes with home on the same branch`,
    expectations: [
      "Committed files are listed (not an empty Changes tab)",
      "The committed-changes toggle is offered",
    ],
  });
});
