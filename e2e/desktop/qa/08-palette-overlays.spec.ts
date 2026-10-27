// The command palette (Ctrl+K), its filtered list, the file picker
// (Ctrl+P), the keyboard shortcuts overlay (?), Go to workspace, the
// prompt history and the branch switcher. The palette and the other
// overlays are dialog windows over the repo window, so they are grabbed
// from the X screen.
import { expect, test } from "@playwright/test";
import { capture } from "./capture.ts";
import { home, mod, openWorkspace, overlay, settle } from "./helpers.ts";

test.describe.configure({ mode: "serial" });

test("palette-overlays: command palette", async ({ page, context }) => {
  await openWorkspace(page, "feat/empty-event-message");
  await page.keyboard.press(`${mod}+k`);
  const palette = await overlay(page, context);
  await expect(palette.getByRole("option", { name: "Go to Home" })).toBeVisible();
  await settle(page, 800);
  await capture(page, {
    name: "08-palette-open",
    title: "Command palette (Ctrl+K) in a workspace",
    screen: true,
    rest: false,
    expectations: [
      "Search field on top; commands with icons and descriptions, first one highlighted",
      "Workspace commands (Toggle Terminal, checks, Open Workspace PR) are listed",
      "Dialog centred over the window with a dimmed backdrop",
    ],
  });
  await palette.getByRole("option").nth(3).hover();
  await settle(page, 400);
  await capture(page, {
    name: "08-palette-row-hover",
    title: "Command palette: a hovered row",
    screen: true,
    rest: false,
    expectations: [
      "The fourth row under the pointer fills with the same grey tint as a hovered sidebar row",
      "The first row keeps its selected highlight",
    ],
  });
  await palette.keyboard.press("ArrowDown");
  await palette.keyboard.press("ArrowDown");
  await settle(page, 400);
  await capture(page, {
    name: "08-palette-keyboard-nav",
    title: "Command palette: keyboard selection moved",
    screen: true,
    rest: false,
    expectations: ["Highlight moved down two rows; list scrolls if needed"],
  });
  await palette.getByRole("textbox", { name: "Type a command or search..." }).fill("term");
  await settle(page, 600);
  await capture(page, {
    name: "08-palette-filtered",
    title: "Command palette filtered by 'term'",
    screen: true,
    rest: false,
    expectations: ["Only terminal commands remain; matched letters emphasised"],
  });
  await palette.getByRole("textbox", { name: "Type a command or search..." }).fill("zzzz-nothing");
  await settle(page, 600);
  await capture(page, {
    name: "08-palette-no-results",
    title: "Command palette with no matches",
    screen: true,
    rest: false,
    expectations: ["A 'No results' empty state, not a blank box"],
  });
  await palette.keyboard.press("Escape");
  await settle(page, 600);
});

test("palette-overlays: file picker", async ({ page }) => {
  await openWorkspace(page, "feat/empty-event-message");
  await page.keyboard.press(`${mod}+p`);
  const search = page.getByRole("textbox", { name: "Search files..." });
  await expect(search).toBeVisible();
  await settle(page, 800);
  await capture(page, {
    name: "08-file-picker-open",
    title: "File picker (Ctrl+P), empty query",
    screen: true,
    rest: false,
    expectations: ["Recent or all files listed with their folders, first highlighted"],
  });
  await search.fill("client");
  await settle(page, 800);
  await capture(page, {
    name: "08-file-picker-results",
    title: "File picker results for 'client'",
    screen: true,
    rest: false,
    expectations: [
      "client.ts and client.test.ts with dimmed paths; match emphasised",
      "Long paths truncate rather than overflow",
    ],
  });
  await search.fill("nothing-matches-this");
  await settle(page, 800);
  await capture(page, {
    name: "08-file-picker-no-results",
    title: "File picker with no matches",
    screen: true,
    rest: false,
    expectations: ["An explicit no-results state"],
  });
  await page.keyboard.press("Escape");
  await expect(search).toHaveCount(0);
});

test("palette-overlays: shortcuts, go to workspace, prompt history, branch switcher", async ({
  page,
  context,
}) => {
  await home(page);
  await page.getByRole("tab", { name: "Commits" }).click();
  await page.keyboard.press("Shift+?");
  const dialog = await overlay(page, context);
  await expect(dialog.getByRole("dialog", { name: "Keyboard shortcuts" })).toBeVisible();
  await settle(page, 800);
  await capture(page, {
    name: "08-shortcuts-overlay",
    title: "Keyboard shortcuts overlay (?)",
    screen: true,
    rest: false,
    expectations: [
      "Groups with key caps aligned in a column; nothing clipped",
      "Key names match the platform (Ctrl on Linux)",
    ],
    issue: "Every shortcut is shown with ⌘ on Linux",
  });
  await dialog.keyboard.press("Escape");
  await settle(page, 600);
  await page.getByRole("tab", { name: "Code" }).click();

  for (const [command, name, title, expectations] of [
    [
      "Go to workspace",
      "08-go-to-workspace",
      "Go to workspace picker",
      ["Workspaces of every repo listed with branch icons; filter field focused"],
    ],
    [
      "View Prompt History",
      "08-prompt-history",
      "Prompt history",
      ["Prompts listed by workspace, or an empty state on a fresh install"],
    ],
    [
      "Switch Branch",
      "08-switch-branch",
      "Switch Branch dialog (home checkout)",
      ["Branches listed with the current one marked; search field on top"],
    ],
    [
      "View Stashed Changes",
      "08-stash-dialog",
      "Stash dialog (no stashes yet)",
      ["Empty state explains stashes; dialog sized to its content"],
    ],
  ] as const) {
    await page.keyboard.press(`${mod}+k`);
    const palette = await overlay(page, context);
    await palette.getByRole("textbox", { name: "Type a command or search..." }).fill(command);
    await settle(page, 400);
    const option = palette.getByRole("option", { name: command });
    if ((await option.count()) === 0) {
      await palette.keyboard.press("Escape");
      continue;
    }
    await option.click();
    await settle(page, 1500);
    await capture(page, { name, title, screen: true, rest: false, expectations: [...expectations] });
    // The overlay's dialog window has the keys.
    await (await overlay(page, context)).keyboard.press("Escape");
    await settle(page, 400);
    await page.keyboard.press("Escape");
    await settle(page, 600);
  }
});
