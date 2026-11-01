// The terminal pane: a shell and an agent session (the fake `claude`
// prints a short transcript), a new shell from the keyboard, search,
// maximize/restore, collapse/expand and the header tooltips.
import { expect, test } from "@playwright/test";
import { capture } from "./capture.ts";
import { mod, openTab, openWorkspace, row, settle } from "./helpers.ts";

const BRANCH = "feat/keyvalues-cache";
const OTHER = "feat/empty-event-message";

test.describe.configure({ mode: "serial" });

test("terminal: shell and agent sessions", async ({ page }) => {
  await openWorkspace(page, BRANCH);
  await openTab(page, "Commits");
  await capture(page, {
    name: "07-terminal-none",
    title: "No terminals yet",
    expectations: ["No terminal chip or pane shows: the tab has the whole height"],
  });
  await expect(page.getByRole("button", { name: "Expand" })).toHaveCount(0);
  // An agent of another workspace, which shares the row with BRANCH's.
  await openWorkspace(page, OTHER);
  await row(page, OTHER).hover();
  await row(page, OTHER).getByRole("button", { name: "Start agent" }).click();
  await settle(page, 3000);
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
      "Terminals follow the theme: dark on #1e1e1e, light on white, with readable ANSI colours",
      "Local, Maximize and Collapse float at the pane's top right",
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
      "Sessions of every workspace share the row, each header naming its workspace",
      "Where the columns overflow the row, a scrollbar under it shows how far it scrolls",
    ],
  });

  // A new shell from the keyboard (Ctrl+\).
  await page.keyboard.press(`${mod}+\\`);
  await settle(page, 2500);
  await page.keyboard.type("echo typed-into-the-new-shell\n");
  await settle(page, 1000);
  await capture(page, {
    name: "07-terminal-new-shell-shortcut",
    title: "Terminal pane after Ctrl+\\ (new shell)",
    expectations: [
      "A new Shell column opens fully in view at the right and takes focus (the typed echo shows in it)",
      "The scrollbar under the row sits at its right end",
    ],
  });
});

test("terminal: tooltip, maximize, collapse", async ({ page }) => {
  await openWorkspace(page, BRANCH);
  await openTab(page, "Commits");
  const maximize = page.getByRole("button", { name: "Maximize" });
  await maximize.hover();
  await settle(page, 1500);
  await capture(page, {
    name: "07-terminal-maximize-tooltip",
    title: "Terminal pane: Maximize tooltip",
    screen: true,
    rest: false,
    expectations: ["Tooltip 'Maximize' with its shortcut, inside the window"],
    issue: "The shortcut reads '⌘ + Ctrl + J' on Linux",
  });
  await maximize.click();
  await settle(page, 1500);
  await capture(page, {
    name: "07-terminal-maximized",
    title: "Terminal pane maximized",
    expectations: [
      "The workspace header stays; the pane fills the area below it",
      "Restore sits in the pane's bar; no control covers terminal text",
    ],
  });
  await page.getByRole("button", { name: "Restore" }).click();
  await settle(page, 1200);
  await capture(page, {
    name: "07-terminal-restored",
    title: "Terminal pane restored",
    expectations: ["The pane is back at its default height"],
  });

  await page.getByRole("button", { name: "Collapse" }).click();
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

test("terminal: agent in a supporting repository", async ({ page }) => {
  const docs = "docs/getting-started";
  await openWorkspace(page, docs);
  await openTab(page, "Commits");
  await row(page, docs).hover();
  await row(page, docs).getByRole("button", { name: "Start agent" }).click();
  await settle(page, 3000);
  await capture(page, {
    name: "07-terminal-supporting-agent",
    title: "Terminal pane: an agent started in docs-site, the supporting repository",
    expectations: [
      "The terminal pane is open with an agent column for docs/getting-started",
      "The fake agent's transcript shows in the column",
    ],
  });
});

// Settings replaces the workspace view; the terminals must be there, all
// of them, when the workspace comes back.
test("terminal: sessions survive a trip to Settings", async ({ page }) => {
  await openWorkspace(page, BRANCH);
  await openTab(page, "Commits");
  await row(page, BRANCH).hover();
  await row(page, BRANCH).getByRole("button", { name: "Open shell" }).click();
  await settle(page, 2500);
  for (let i = 0; i < 2; i++) {
    await row(page, BRANCH).hover();
    await row(page, BRANCH).getByRole("button", { name: "Start agent" }).click();
    await settle(page, 3000);
  }
  await capture(page, {
    name: "07-terminal-settings-01-before",
    title: "Terminal pane: a shell and two agents, before Settings",
    expectations: ["A shell and two agent columns show"],
  });
  await page.getByRole("button", { name: "Settings", exact: true }).click();
  const close = page.getByRole("button", { name: "Back", exact: true });
  await expect(close).toBeVisible();
  await settle(page, 1000);
  await close.click();
  await expect(page.getByRole("tab", { name: "Commits" })).toBeVisible();
  await settle(page, 2500);
  await capture(page, {
    name: "07-terminal-settings-02-after",
    title: "Terminal pane after opening and closing Settings",
    expectations: [
      "The same shell and two agent columns show, with their output",
      "No 'PTY session creation already in progress' message",
    ],
  });
});
