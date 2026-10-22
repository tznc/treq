// The terminal pane: a shell and an agent session (the fake `claude`
// prints a short transcript), a new shell from the keyboard, search,
// maximize/restore, collapse/expand and the header tooltips.
//
// The pane's header controls are not observable: the environment chip
// (Local, Maximize, Collapse) sits at the pane's top right, at a fixed
// place in the 1400x900 window while the pane has its default height.
import { expect, test } from "@playwright/test";
import { capture } from "./capture.ts";
import { mod, openTab, openWorkspace, row, settle } from "./helpers.ts";

const BRANCH = "feat/keyvalues-cache";
const CHIP_Y = 589;
const MAXIMIZE_X = 1359;
const COLLAPSE_X = 1381;

test.describe.configure({ mode: "serial" });

test("terminal: shell and agent sessions", async ({ page }) => {
  await openWorkspace(page, BRANCH);
  await openTab(page, "Commits");
  await row(page, BRANCH).hover();
  await row(page, BRANCH).getByRole("button", { name: "Open shell" }).click();
  await settle(page, 2500);
  await page.mouse.click(1000, 780); // focus the new shell (rightmost column)
  await page.keyboard.type("PS1='event-bus $ '; clear\n");
  await settle(page, 800);
  await page.keyboard.type("ls -la packages/api/src && git log --oneline -3\n");
  await settle(page, 1500);
  await capture(page, {
    name: "07-terminal-shell",
    title: "Terminal pane: a shell session",
    expectations: [
      "Each session column has a header naming it and its workspace",
      "Prompt, ls output and git log colours render in the theme's ANSI palette",
      "Cursor visible; no stray characters",
    ],
  });
  await capture(page, {
    name: "07-terminal-shell-header-selected",
    title: "Terminal pane: the selected shell's header",
    clip: { x: 0, y: 575, width: 1400, height: 90 },
    expectations: [
      "The selected Shell header's title, terminal icon and close × read clearly on its blue fill",
      "The workspace badge's text is legible against its chip",
    ],
  });

  await row(page, BRANCH).hover();
  await row(page, BRANCH).getByRole("button", { name: "Start agent" }).click();
  await settle(page, 3000);
  await capture(page, {
    name: "07-terminal-agent",
    title: "Terminal pane: an agent session beside the shell",
    expectations: [
      "Agent transcript: bold prompt line, green bullets, dimmed sub-lines",
      "Columns share the pane's width; none is cut off at the window edge",
      "Only this workspace's sessions show",
    ],
    issue:
      "Sessions of other workspaces (AI Review of feat/empty-event-message) share the pane, and the last column is cut off at the right edge",
  });

  // A new shell from the keyboard (Ctrl+\).
  await page.keyboard.press(`${mod}+\\`);
  await settle(page, 2500);
  await capture(page, {
    name: "07-terminal-new-shell-shortcut",
    title: "Terminal pane after Ctrl+\\ (new shell)",
    expectations: ["A new Shell column opens and takes focus"],
    issue: "No new column is visible: the pane already overflows its width",
  });
});

test("terminal: tooltip, maximize, collapse", async ({ page }) => {
  await openWorkspace(page, BRANCH);
  await openTab(page, "Commits");
  await page.mouse.move(MAXIMIZE_X, CHIP_Y, { steps: 4 });
  await settle(page, 1500);
  await capture(page, {
    name: "07-terminal-maximize-tooltip",
    title: "Terminal pane: Maximize tooltip",
    screen: true,
    rest: false,
    expectations: ["Tooltip 'Maximize' with its shortcut, inside the window"],
    issue: "The shortcut reads '⌘ + Ctrl + J' on Linux",
  });
  await page.mouse.click(MAXIMIZE_X, CHIP_Y);
  await settle(page, 1500);
  await capture(page, {
    name: "07-terminal-maximized",
    title: "Terminal pane maximized",
    expectations: [
      "The pane fills the content area below the workspace header",
      "A Restore control replaces Maximize; terminals reflow",
    ],
  });
  // Restore: the chip moves with the pane's top edge, so use the keyboard.
  await page.keyboard.press("Meta+Control+j");
  await settle(page, 1200);
  await capture(page, {
    name: "07-terminal-restored",
    title: "Terminal pane restored (Super+Ctrl+J)",
    expectations: ["The pane is back at its default height"],
  });

  await page.mouse.click(COLLAPSE_X, CHIP_Y);
  await settle(page, 1200);
  await capture(page, {
    name: "07-terminal-collapsed",
    title: "Terminal pane collapsed",
    expectations: [
      "The pane folds to its chip at the bottom right; the tab regains the space",
      "An Expand control remains reachable",
    ],
  });
  await page.mouse.click(700, 135); // empty space beside the tab row
  await page.keyboard.press(`${mod}+j`);
  await settle(page, 1200);
  await capture(page, {
    name: "07-terminal-expanded-shortcut",
    title: "Terminal pane expanded again (Ctrl+J)",
    expectations: ["The pane and its sessions come back as they were"],
  });
  await expect(page.getByRole("tab", { name: "Commits", selected: true })).toBeVisible();
});

// The fake agent draws a rule as wide as its PTY and redraws it in place
// on a resize, as a prompt or a TUI does. Narrowing its column must not
// leave the old rule's head behind.
test("terminal: a narrower column redraws in place", async ({ page }) => {
  await openWorkspace(page, BRANCH);
  await openTab(page, "Commits");
  await row(page, BRANCH).hover();
  await row(page, BRANCH).getByRole("button", { name: "Open shell" }).click();
  await settle(page, 2500);
  await page.mouse.click(1000, 780);
  await page.keyboard.type("clear; claude\n");
  await settle(page, 2000);
  await capture(page, {
    name: "07-terminal-redraw-wide",
    title: "Terminal pane: a program drawing a full-width rule",
    expectations: ["The dimmed ==== rule is one line ending at the right edge"],
  });
  await row(page, BRANCH).hover();
  await row(page, BRANCH).getByRole("button", { name: "Start agent" }).click();
  await settle(page, 3000);
  await capture(page, {
    name: "07-terminal-redraw-narrow",
    title: "Terminal pane: the same program after an agent opens beside it",
    expectations: [
      "In the narrowed column the rule is one line ending at the column's edge",
      "No leftover rule fragment sits above it",
    ],
  });
});
