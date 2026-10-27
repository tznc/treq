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
import { execFileSync } from "node:child_process";
import { dismissPopover, openWorkspace, settle, xPointer } from "./helpers.ts";

test.describe.configure({ mode: "serial" });

const BRANCH = "feat/keyvalues-cache";
const HEADER_Y = 24;

/** Whether a window besides the repo window shows: the popover's. */
function popoverShown(): boolean {
  return (
    execFileSync("xdotool", ["search", "--onlyvisible", "--name", "."])
      .toString()
      .trim()
      .split("\n")
      .filter(Boolean).length > 1
  );
}

async function popupShot(
  page: Page,
  open: () => Promise<void>,
  shot: Parameters<typeof capture>[1],
) {
  await open();
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
      "Branch → base picker, Details (info icon); Checks 3/3, View PR + external link, Merge…, ⋮",
      "Buttons share one height and baseline; nothing wraps",
    ],
  });
  await popupShot(page, () => page.mouse.click(552, HEADER_Y), {
    name: "12-merge-base-picker",
    title: "Base branch picker",
    expectations: ["Branches searchable; the current base (main) marked"],
  });
  await popupShot(page, () => page.getByRole("button", { name: /^Checks:/ }).click(), {
    name: "12-merge-checks-popover",
    title: "Checks popover (GH 3/3)",
    expectations: [
      "Each check with status icon, name and duration; WIP last, muted, reading inactive",
      "Popover stays inside the window at the right edge",
    ],
  });
  await page.getByRole("button", { name: "Workspace details" }).click();
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
  });
  await dismissPopover(page);
  await popupShot(page, () => page.getByRole("button", { name: "More workspace actions" }).click(), {
    name: "12-merge-overflow-menu",
    title: "Workspace header overflow menu (⋮)",
    expectations: ["Items aligned with icons; destructive ones last and red"],
  });
});

test("merge: popovers close with Escape and let the next click through", async ({ page }) => {
  await openWorkspace(page, BRANCH);
  // Escape in the repo window closes each popover.
  const open = {
    "base-picker": () => page.mouse.click(552, HEADER_Y),
    checks: () => page.getByRole("button", { name: /^Checks:/ }).click(),
    details: () => page.getByRole("button", { name: "Workspace details" }).click(),
  };
  for (const [what, click] of Object.entries(open)) {
    await click();
    await settle(page, 1200);
    expect(popoverShown(), `${what} popover open`).toBe(true);
    await page.keyboard.press("Escape");
    await settle(page, 800);
    await capture(page, {
      name: `12-merge-escape-${what}`,
      title: `After Escape on the ${what} popover`,
      screen: true,
      rest: false,
      expectations: ["The popover is closed; the workspace header shows unchanged"],
    });
    expect.soft(popoverShown(), `${what} popover closed by Escape`).toBe(false);
  }
  // A click outside the Details popover closes it and still reaches the tab.
  await page.getByRole("button", { name: "Workspace details" }).click();
  await settle(page, 1200);
  expect(popoverShown()).toBe(true);
  const changes = await page.getByRole("tab", { name: "Changes" }).boundingBox();
  if (!changes) throw new Error("no Changes tab");
  await xPointer(page, changes.x + changes.width / 2, changes.y + changes.height / 2, 1);
  await settle(page, 1500);
  await capture(page, {
    name: "12-merge-outside-click",
    title: "After clicking the Changes tab with Details open",
    screen: true,
    rest: false,
    expectations: [
      "The Details popover is closed",
      "The Changes tab is selected and its file list shows",
    ],
  });
  expect.soft(popoverShown()).toBe(false);
  await expect(page.getByRole("tab", { name: "Changes", selected: true })).toBeVisible();
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
