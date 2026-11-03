// The home view and the sidebar: two repositories (event-bus and its
// supporting docs-site), stacked workspaces, hover actions, tooltips,
// context menus and multi-select with the bulk archive bar.
import { expect, test } from "@playwright/test";
import { capture } from "./capture.ts";
import { home, openWorkspace, ready, row, settle } from "./helpers.ts";

test.describe.configure({ mode: "serial" });

test("home-sidebar: home view, rows, menus and multi-select", async ({ page }) => {
  await ready(page);
  // The PR status poll fills the stack's PR and CI state.
  await settle(page, 3000);
  await home(page);
  await capture(page, {
    name: "02-home-view",
    title: "Home view (event-bus main) with the sidebar",
    expectations: [
      "Sidebar: event-bus and docs-site home rows, Github, Linear, then WORKSPACES grouped by repo",
      "Stacked feat/empty-event-message is indented under feat/event-ingest; status dots aligned right",
      "Code tab: Agent and Go to file in one row, file tree, README preview and the Commits rail; no prompt box",
    ],
    issue: "Shortcut hints read '⌘ + K' / '⌘ + P' on Linux; the home file tree lists .git",
  });

  await row(page, "feat/keyvalues-cache").hover();
  await settle(page, 1500);
  await capture(page, {
    name: "02-sidebar-row-hover-tooltip",
    title: "Workspace row hover: actions and PR tooltip",
    screen: true,
    rest: false,
    expectations: [
      "Hovered row shows Start agent, Open shell and Stack on this… icons, aligned",
      "Tooltip names the branch and its PR state (Open PR) and does not cover the row",
    ],
  });

  await row(page, "feat/keyvalues-cache")
    .getByRole("button", { name: "Stack on this…" })
    .hover();
  await settle(page, 1500);
  await capture(page, {
    name: "02-sidebar-action-tooltip",
    title: "Tooltip on a row action button",
    screen: true,
    rest: false,
    expectations: ["The Stack on this… icon's tooltip is readable and placed next to it"],
  });

  await row(page, "main").click({ button: "right" });
  await settle(page, 1000);
  await capture(page, {
    name: "02-sidebar-home-context-menu",
    title: "Home row context menu",
    screen: true,
    rest: false,
    expectations: [
      "Menu opens at the pointer with icons and labels aligned",
      "Separators and destructive items are styled consistently",
    ],
  });
  await page.keyboard.press("Escape");
  await settle(page, 500);

  await row(page, "feat/alerting-logs").click({ button: "right" });
  await settle(page, 1000);
  await capture(page, {
    name: "02-sidebar-workspace-context-menu",
    title: "Workspace row context menu",
    screen: true,
    rest: false,
    expectations: [
      "Copy branch name, Copy link to GitHub PR, Rename, Copy paths, Open in…, Archive",
      "Archive Workspace is red and set apart by a separator",
    ],
  });
  // The "Open in…" submenu is not captured: menus are popup windows only
  // real pointer events reach, and under Xvfb with no window manager any
  // real pointer motion into the popup closes it.

  // Multi-select: one click, then Ctrl+click a second row.
  await openWorkspace(page, "feat/keyvalues-cache");
  await row(page, "feat/alerting-logs").click({ modifiers: ["Control"] });
  await settle(page, 800);
  await expect(
    page.getByRole("button", { name: "feat/alerting-logs", exact: true, selected: true }),
  ).toBeVisible();
  await capture(page, {
    name: "02-sidebar-multi-select-bulk-archive",
    title: "Two workspaces multi-selected, bulk archive bar",
    expectations: [
      "Both selected rows are highlighted",
      "Bulk archive button counts the selection ('Archive 2 workspaces')",
    ],
  });

  // Shift+click extends a range across the repo's list.
  await row(page, "feat/event-ingest").click({ modifiers: ["Shift"] });
  await settle(page, 800);
  await capture(page, {
    name: "02-sidebar-range-select",
    title: "Shift+click range selection",
    expectations: ["Every row between the anchor and the click is selected; the bar's count matches"],
  });
  // Clicking a row alone clears the selection.
  await openWorkspace(page, "feat/event-ingest");

  // The supporting repository's home.
  const docsHome = page.getByRole("button", { name: /docs-site/ }).first();
  if ((await docsHome.count()) > 0) {
    await docsHome.click();
  } else {
    // The docs-site home row has no accessible name; it is the row under event-bus.
    const main = await row(page, "main").boundingBox();
    if (main) await page.mouse.click(main.x + 60, main.y + main.height + 16);
  }
  await settle(page, 2500);
  await capture(page, {
    name: "02-home-supporting-repo",
    title: "Home view of the supporting repository docs-site",
    expectations: [
      "Header and Code tab show docs-site, not event-bus",
      "Sidebar marks the docs-site home row as selected",
      "docs-site's README and single commit show",
    ],
  });
  await row(page, "docs/getting-started").click();
  await settle(page, 2000);
  await capture(page, {
    name: "02-supporting-repo-workspace",
    title: "Workspace of the supporting repository",
    expectations: [
      "Header names docs/getting-started on docs-site's main",
      "Tabs and counts match an empty workspace (no commits, no changes)",
    ],
  });
  await home(page);
});
