// The Code tab of a stacked, conflicted workspace: the action row (Agent,
// the alerts button and its popover, Go to file), the stack panel, the file
// tree and the file view (code and Markdown preview), plus one non-default
// zoom.
//
// File tree rows are not observable elements, so those steps click a point
// found from an observable neighbour (the window is a fixed 1400x900 and the
// seed deterministic).
import { expect, test, type Page } from "@playwright/test";
import { capture } from "./capture.ts";
import { home, mod, openTab, openWorkspace, settle, xPointer } from "./helpers.ts";

const BRANCH = "feat/empty-event-message";

test.describe.configure({ mode: "serial" });

/** Leaves the file view (its "Back" row is not observable) for the overview. */
async function overview(page: Page) {
  const goToFile = page.getByRole("button", { name: "Go to file" });
  if ((await goToFile.count()) === 0) {
    await page.mouse.click(325, 123);
    await expect(goToFile).toBeVisible();
    await settle(page, 800);
  }
}

test("code: workspace Code tab, action row, alerts and Agent", async ({ page, context }) => {
  await openWorkspace(page, BRANCH);
  await openTab(page, "Code");
  await overview(page);
  await settle(page, 2000);
  await expect(page.getByRole("textbox", { name: "Describe a task…" })).toHaveCount(0);
  await expect(page.getByRole("alert", { name: "1 file has conflicts" })).toHaveCount(0);
  await capture(page, {
    name: "03-code-workspace",
    title: "Code tab of a stacked workspace with a conflict",
    expectations: [
      "Header: branch → base picker, View PR, a checks pill reading 'N of M checks failing' (red), Merge… disabled",
      "Under the title card one row: Agent on the left; an alerts button (red icon and count) then Go to file on the right",
      "No inline prompt card and no alert banners; the stack panel follows the action row",
    ],
  });

  const alerts = page.getByRole("button", { name: /^Alerts/ });
  await alerts.click();
  const conflicts = page.getByRole("alert", { name: "1 file has conflicts" });
  await expect(conflicts).toBeVisible();
  await settle(page, 600);
  await capture(page, {
    name: "03-code-alerts-popover",
    title: "Code tab: the alerts popover",
    expectations: [
      "A popover under the alerts button lists each alert as its own card",
      "The red '1 file has conflicts' alert keeps its Resolve… action",
    ],
  });
  await page.keyboard.press("Escape");
  await expect(conflicts).toHaveCount(0);

  await page.getByRole("button", { name: "Agent", exact: true }).click();
  const title = (p: Page) => p.getByRole("dialog", { name: "Start a new agent session" });
  await expect(title(page)).toBeVisible();
  const dialog = page;
  await expect(dialog.getByRole("button", { name: "Send" })).toBeVisible();
  await expect(dialog.getByRole("button", { name: "More submit options" })).toHaveCount(0);
  await settle(page, 800);
  await capture(page, {
    name: "03-code-agent-prompt",
    title: "Agent opens the agent prompt",
    screen: true,
    rest: false,
    expectations: [
      "The Start a new agent session dialog is open on this workspace",
      "Its footer ends in a Send icon button, with no Edit/Plan split menu",
    ],
  });
  await page.keyboard.press("Escape");
  await settle(page, 600);

  // The stack panel's help icon, right of its "Stack" title (not
  // observable; its place below the action row).
  await page.mouse.move(383, 367, { steps: 4 });
  await settle(page, 1500);
  await capture(page, {
    name: "03-code-stack-help-tooltip",
    title: "Stack panel help tooltip",
    screen: true,
    rest: false,
    expectations: ["Tooltip explains stacks and is fully on screen"],
  });

  // The file tree: scroll to it and open README.md, the last row.
  await page.mouse.move(800, 700);
  await page.mouse.wheel(0, 600);
  await settle(page, 800);
  await capture(page, {
    name: "03-code-tree-and-readme",
    title: "Code tab scrolled: file tree and README preview",
    expectations: [
      "Tree rows (packages, .gitignore, README.md) aligned; names in the neutral text colour, packages has a changed dot after its name",
      "README card renders headings, inline code and bullets",
    ],
  });
});

test("code: file view, Markdown preview and a TSX file", async ({ page }) => {
  await openWorkspace(page, BRANCH);
  await openTab(page, "Code");
  // Open README.md from the picker: the tree rows are not observable.
  await page.keyboard.press(`${mod}+p`);
  const search = page.getByRole("textbox", { name: "Go to file…" });
  await expect(search).toBeVisible();
  await search.fill("README");
  await settle(page, 800);
  await page.keyboard.press("Enter");
  await settle(page, 1500);
  await capture(page, {
    name: "03-code-file-view-markdown",
    title: "File view: README.md as code",
    expectations: [
      "Tree on the left (packages expanded to api/web), README.md highlighted",
      "Line numbers, Markdown highlighting, 'Modified … ago', Code/Preview toggle",
    ],
  });
  const preview = page.getByRole("button", { name: "Preview" });
  if ((await preview.count()) > 0) {
    await preview.click();
  } else {
    await page.mouse.click(1310, 164);
  }
  await settle(page, 1000);
  await capture(page, {
    name: "03-code-file-view-preview",
    title: "File view: README.md Markdown preview",
    expectations: ["Rendered Markdown; Preview segment selected in the toggle"],
  });

  await page.keyboard.press(`${mod}+p`);
  await expect(search).toBeVisible();
  await search.fill("Home.tsx");
  await settle(page, 800);
  await page.keyboard.press("Enter");
  await settle(page, 1500);
  await capture(page, {
    name: "03-code-file-view-tsx",
    title: "File view: a conflicted TSX file",
    expectations: [
      "TSX syntax colours readable in this theme",
      "Conflict markers or conflict state shown for Home.tsx",
      "Tree expanded to packages/web/src/pages with Home.tsx selected",
    ],
  });

  // A copy button turns into a green check, then back.
  await page.getByRole("button", { name: "Copy file path" }).click();
  await settle(page, 400);
  await capture(page, {
    name: "03-code-file-view-path-copied",
    title: "File view: file path copied",
    expectations: ["The Copy file path button beside the path shows a green check"],
  });
  await settle(page, 2500);
  await capture(page, {
    name: "03-code-file-view-path-copy-reverted",
    title: "File view: copy button reverted",
    expectations: ["The Copy file path button shows the copy icon again"],
  });
});

test("code: zoom 120%", async ({ page }) => {
  await openWorkspace(page, BRANCH);
  await openTab(page, "Code");
  await overview(page);
  await page.keyboard.press(`${mod}+=`);
  await page.keyboard.press(`${mod}+=`);
  await settle(page, 1500);
  try {
    await capture(page, {
      name: "03-code-workspace-zoom-in",
      title: "Code tab at a larger UI zoom (Ctrl+= twice)",
      expectations: [
        "Everything scales together: sidebar, header, tabs, text",
        "Nothing clips or overlaps at the larger size",
      ],
    });
  } finally {
    await page.keyboard.press(`${mod}+-`);
    await page.keyboard.press(`${mod}+-`);
    await settle(page, 1000);
  }
});

// The home Code tab's Commits column. Its rows are not observable; the
// first one sits under the "Today" label at the column's top.
test("code: commit history hover", async ({ page }) => {
  await home(page);
  await openTab(page, "Code");
  await settle(page, 1500);
  for (const [name, y, expectations] of [
    ["03-code-history-hover-first", 210, [
      "Hovered first commit: one even card, no shadow showing through its fill",
      "Card fill matches the row highlight behind the dot (no darker inner card)",
    ]],
    ["03-code-history-hover-other", 295, [
      "Hovered second commit: row highlight behind dot and text, same tone throughout",
    ]],
  ] as const) {
    await page.mouse.move(1288, y);
    await xPointer(page, 1288, y);
    await settle(page, 500);
    await capture(page, { name, title: "Code tab commit history: hovered row", rest: false, expectations: [...expectations] });
  }
});

test("code: workspace title above the description, edited inline", async ({ page }) => {
  await openWorkspace(page, BRANCH);
  await openTab(page, "Code");
  await overview(page);
  await settle(page, 1500);
  const edit = page.getByRole("button", { name: "Edit workspace title" });
  await expect(edit).toBeVisible();
  await capture(page, {
    name: "03-code-title",
    title: "Code tab: full workspace title with its edit pencil",
    expectations: [
      "The workspace title sits at the top of the Code tab as a large heading, wrapped rather than truncated",
      "A small pencil button sits at the title's top right",
      "The header no longer repeats the title under the branch row",
    ],
  });

  await edit.click();
  const input = page.getByRole("textbox", { name: "Workspace title" });
  await expect(input).toBeVisible();
  await capture(page, {
    name: "03-code-title-editing",
    title: "Code tab: editing the workspace title",
    expectations: [
      "The title becomes a focused editor holding the current title",
      "Cancel and Save buttons sit right-aligned under the editor",
    ],
  });
  await page.keyboard.press("Escape");
  await expect(input).toHaveCount(0);
  await home(page);
});
