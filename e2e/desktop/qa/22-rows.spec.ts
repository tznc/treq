// Every selectable list in the app draws its rows with `ui::Row`: one
// height per density, the hover token on hover and the light selected tint
// (no shadow, text not recoloured) on the selected row. One capture per
// list: the sidebar, the Changes file list, commits, GitHub's pull
// requests, the New Workspaces picker, the palette, the Skills settings and
// the Logs level menu.
import { expect, test, type Page } from "@playwright/test";
import { capture } from "./capture.ts";
import { dismissPopover, home, mod, openTab, openWorkspace, overlay, ready, row, settle } from "./helpers.ts";

test.describe.configure({ mode: "serial" });

/** events.json in feat/alerting-logs's Committed list. */
const CHANGES_ROW = { x: 330, y: 351 };
/** The GitHub view's Pull Requests tab and first row (10-github-linear). */
const PRS_TAB = { x: 422, y: 62 };
const FIRST_PR = { x: 700, y: 147 };

const SELECTED = "The selected row uses the light accent tint, with no shadow, and its text is not coloured";

test("rows: sidebar", async ({ page }) => {
  await openWorkspace(page, "feat/event-ingest");
  const box = await row(page, "feat/event-ingest").boundingBox();
  if (!box) throw new Error("row not drawn");
  await capture(page, {
    name: "22-rows-sidebar",
    title: "Sidebar: home, nav and workspace rows",
    clip: { x: 0, y: 0, width: box.x + box.width + 8, height: 520 },
    expectations: [
      `feat/event-ingest is selected. ${SELECTED}`,
      "Home, GitHub, Linear and workspace rows share one height",
    ],
  });
});

test("rows: changes file list", async ({ page }) => {
  await openWorkspace(page, "feat/alerting-logs");
  await openTab(page, "Changes");
  await page.mouse.click(CHANGES_ROW.x, CHANGES_ROW.y);
  await settle(page, 1500);
  await page.mouse.move(1000, 860);
  await capture(page, {
    name: "22-rows-changes",
    title: "Changes: file list with a selected file",
    clip: { x: 0, y: 0, width: 520, height: 520 },
    expectations: [
      `events.json is selected. ${SELECTED}`,
      "File rows share one height; the status letter sits at the right edge",
    ],
  });
});

test("rows: commits", async ({ page }) => {
  await openWorkspace(page, "feat/keyvalues-cache");
  await openTab(page, "Commits");
  await settle(page, 1500);
  await capture(page, {
    name: "22-rows-commits",
    title: "Commits: the commit list",
    expectations: [
      "Commit rows line up: dot, chevron, headline, then time and +/− counts",
      "No row has a shadow or a coloured headline",
    ],
  });
});

test("rows: github pull requests", async ({ page }) => {
  await ready(page);
  await page.getByRole("button", { name: "Github", exact: true }).click();
  await expect(page.getByRole("button", { name: "Github", selected: true })).toBeVisible();
  await settle(page, 2000);
  await page.mouse.click(PRS_TAB.x, PRS_TAB.y);
  await settle(page, 2000);
  await page.mouse.click(FIRST_PR.x, FIRST_PR.y);
  await settle(page, 2500);
  await capture(page, {
    name: "22-rows-github-prs",
    title: "GitHub: pull requests with one selected",
    expectations: [
      `The first pull request is selected. ${SELECTED}`,
      "Rows are separated by a hairline and share one padding",
    ],
  });
});

test("rows: open workspaces picker", async ({ page }) => {
  await home(page);
  await page.getByRole("button", { name: "New workspaces" }).click();
  await expect(page.getByRole("heading", { name: "Your pull requests" })).toBeVisible();
  await page.getByRole("checkbox", { name: /^#418/ }).click();
  await page.mouse.move(1000, 860);
  await settle(page, 800);
  await capture(page, {
    name: "22-rows-open-workspaces",
    title: "New Workspaces: a checked pull request",
    expectations: [
      `#418 is checked. ${SELECTED}`,
      "Pull request rows share one height, each with its checkbox on the left",
    ],
  });
  await page.keyboard.press("Escape");
});

test("rows: palette", async ({ page, context }) => {
  await openWorkspace(page, "feat/empty-event-message");
  await page.keyboard.press(`${mod}+k`);
  const palette = await overlay(page, context);
  await expect(palette.getByRole("option", { name: "Go to Home" })).toBeVisible();
  await palette.keyboard.press("ArrowDown");
  await settle(page, 800);
  await capture(page, {
    name: "22-rows-palette",
    title: "Command palette: the second command selected",
    screen: true,
    rest: false,
    expectations: [
      `The second command is selected. ${SELECTED}`,
      "Command rows share one height, icon then label and description",
    ],
  });
  await palette.keyboard.press("Escape");
});

async function settings(page: Page, tab: string): Promise<void> {
  await ready(page);
  await page.getByRole("button", { name: "Settings", exact: true }).click();
  await page.getByRole("tab", { name: tab }).click();
  await expect(page.getByRole("tab", { name: tab, selected: true })).toBeVisible();
  await page.mouse.move(1000, 860);
  await settle(page, 1500);
}

test("rows: settings skills", async ({ page }) => {
  await settings(page, "Skills");
  await capture(page, {
    name: "22-rows-settings-skills",
    title: "Settings: Skills list",
    expectations: [
      "Skills listed as cards with name, labels, description and actions",
      "Nothing in the list has a coloured selected fill",
    ],
  });
});

test("rows: logs level menu", async ({ page }) => {
  await openWorkspace(page, "feat/keyvalues-cache");
  await openTab(page, "Logs");
  await page.mouse.click(360, 183);
  await settle(page, 900);
  await capture(page, {
    name: "22-rows-logs-levels",
    title: "Logs: the level filter menu",
    screen: true,
    rest: false,
    expectations: [
      "All Levels is checked; level rows share one height",
      "Check marks sit in the left gutter, labels line up",
    ],
  });
  await dismissPopover(page);
});
