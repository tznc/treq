// The workspace header's merge controls on feat/keyvalues-cache (open PR
// #401, checks 3/3): the base branch picker, the Checks popover, Details,
// the overflow menu and the Merge Preview with its strategies. Nothing
// is merged: the preview is left with Back.
//
// The header's View PR, Checks, base branch and overflow buttons have no
// accessible names; they are clicked at their place in the 1400x900
// window (the header row at y=24).
import { expect, test, type Page } from "@playwright/test";
import { capture } from "./capture.ts";
import { dismissPopover, openWorkspace, settle } from "./helpers.ts";

test.describe.configure({ mode: "serial" });

const BRANCH = "feat/keyvalues-cache";
const HEADER_Y = 24;

async function popupShot(page: Page, x: number, shot: Parameters<typeof capture>[1]) {
  await page.mouse.click(x, HEADER_Y);
  await settle(page, 1200);
  await capture(page, { screen: true, rest: false, ...shot });
  await dismissPopover(page);
}

test("merge: header popovers and menus", async ({ page }) => {
  await openWorkspace(page, BRANCH);
  await capture(page, {
    name: "12-merge-header",
    title: "Workspace header with an open PR and green checks",
    expectations: [
      "Branch → base picker, View PR + external link, Checks GH 3/3, Merge…, Details, ⋮",
      "Buttons share one height and baseline; nothing wraps",
    ],
  });
  await popupShot(page, 552, {
    name: "12-merge-base-picker",
    title: "Base branch picker",
    expectations: ["Branches searchable; the current base (main) marked"],
  });
  await popupShot(page, 1068, {
    name: "12-merge-checks-popover",
    title: "Checks popover (GH 3/3)",
    expectations: [
      "Each check with status icon, name and duration",
      "Popover stays inside the window at the right edge",
    ],
  });
  await page.getByRole("button", { name: "Details" }).click();
  await settle(page, 1200);
  await capture(page, {
    name: "12-merge-details",
    title: "Workspace Details popover",
    screen: true,
    rest: false,
    expectations: [
      "Path, branch, base, PR and created time with copy buttons",
      "Long paths truncate in the middle or wrap; popover inside the window",
    ],
    issue: "Escape does not close the popover; its window-wide backdrop eats the next click",
  });
  await dismissPopover(page);
  await popupShot(page, 1380, {
    name: "12-merge-overflow-menu",
    title: "Workspace header overflow menu (⋮)",
    expectations: ["Items aligned with icons; destructive ones last and red"],
  });
});

test("merge: merge preview and strategies", async ({ page }) => {
  await openWorkspace(page, BRANCH);
  await page.getByRole("button", { name: "Merge..." }).click();
  const back = page.getByRole("button", { name: "Back" });
  await expect(back).toBeVisible();
  await settle(page, 2500);
  await capture(page, {
    name: "12-merge-preview",
    title: "Merge Preview for feat/keyvalues-cache into main",
    expectations: [
      "Back, title, source → target; strategy choices with descriptions",
      "Files to merge with +/- counts; primary button names the strategy",
    ],
  });
  // The strategy choices: Squash and merge, Rebase and merge (by text).
  for (const [label, name] of [
    ["Squash and merge", "12-merge-preview-squash"],
    ["Rebase and merge", "12-merge-preview-rebase"],
  ] as const) {
    const choice = page.getByText(label).first();
    try {
      await choice.click({ timeout: 3000 });
    } catch {
      continue;
    }
    await settle(page, 1000);
    await capture(page, {
      name,
      title: `Merge Preview: ${label} selected`,
      expectations: [
        "The selected strategy is marked; the commit message field matches it",
        "The primary button's label follows the strategy",
      ],
    });
  }
  await page.mouse.move(900, 600);
  await page.mouse.wheel(0, 800);
  await settle(page, 1000);
  await capture(page, {
    name: "12-merge-preview-files",
    title: "Merge Preview: the files and diff below",
    expectations: ["File diffs render under the strategy card; no overlap"],
  });
  await back.click();
  await expect(page.getByRole("button", { name: "Merge..." })).toBeVisible();
});
