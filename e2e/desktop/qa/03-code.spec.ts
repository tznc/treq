// The Code tab of a stacked, conflicted workspace: the conflict banner,
// the prompt box and its menus, the stack panel, the file tree and the
// file view (code and Markdown preview), plus one non-default zoom.
//
// File tree rows and the prompt box's menu buttons are not observable
// elements, so those steps click a point found from an observable
// neighbour (the window is a fixed 1400x900 and the seed deterministic).
import { expect, test, type Page } from "@playwright/test";
import { capture } from "./capture.ts";
import { home, mod, openTab, openWorkspace, settle, xPointer } from "./helpers.ts";

const BRANCH = "feat/empty-event-message";

test.describe.configure({ mode: "serial" });

async function promptRow(page: Page) {
  const edit = await page.getByRole("button", { name: "Edit", exact: true }).boundingBox();
  const box = await page.getByRole("textbox", { name: "Describe a task…" }).boundingBox();
  if (!edit || !box) throw new Error("prompt box not on screen");
  return { y: edit.y + edit.height / 2, left: box.x, editX: edit.x };
}

/** Leaves the file view (its "Back" row is not observable) for the overview. */
async function overview(page: Page) {
  const prompt = page.getByRole("textbox", { name: "Describe a task…" });
  if ((await prompt.count()) === 0) {
    await page.mouse.click(325, 123);
    await expect(prompt).toBeVisible();
    await settle(page, 800);
  }
}

async function popup(page: Page, x: number, y: number, name: string, title: string, expectations: string[]) {
  await page.mouse.click(x, y);
  await settle(page, 1000);
  await capture(page, { name, title, screen: true, rest: false, expectations });
  await page.keyboard.press("Escape");
  await settle(page, 500);
}

test("code: workspace Code tab, prompt menus, tree and file view", async ({ page }) => {
  await openWorkspace(page, BRANCH);
  await openTab(page, "Code");
  await overview(page);
  await settle(page, 2000);
  await capture(page, {
    name: "03-code-workspace",
    title: "Code tab of a stacked workspace with a conflict",
    expectations: [
      "Header: branch → base picker, View PR, Checks GH 2/4 (red), Merge… disabled, Details, overflow",
      "Red '1 file has conflicts' alert with Resolve…; tab badges Commits 2 / Changes 3",
      "Stack panel lists feat/empty-event-message over feat/event-ingest over main, with +/- bars",
    ],
  });

  const prompt = page.getByRole("textbox", { name: "Describe a task…" });
  await prompt.fill("Handle empty event_message in postEvent and add a regression test");
  await capture(page, {
    name: "03-code-prompt-typed",
    title: "Prompt box with text",
    expectations: [
      "Typed text wraps inside the box; placeholder gone",
      "Edit button becomes enabled (primary colour)",
    ],
  });

  const { y, left, editX } = await promptRow(page);
  await popup(page, left + 30, y, "03-code-prompt-add-menu", "Prompt box: Add menu", [
    "Menu opens under Add with its items aligned and readable",
  ]);
  await popup(page, editX - 55, y, "03-code-agent-picker", "Prompt box: agent picker (Claude)", [
    "Agent list (Claude, Codex, …) with icons; the current one is marked",
    "Uninstalled agents are disabled or labelled",
  ]);
  await popup(page, editX - 150, y, "03-code-prompt-target-menu", "Prompt box: Local / remote target menu", [
    "Target options readable; the current one is marked",
  ]);
  await page.getByRole("button", { name: "More submit options" }).click();
  await settle(page, 1000);
  await capture(page, {
    name: "03-code-submit-split-menu",
    title: "Prompt box: Edit split-button menu",
    screen: true,
    rest: false,
    expectations: ["Submit modes (Edit/Plan/…) listed under the split arrow, aligned"],
  });
  await page.keyboard.press("Escape");
  await prompt.fill("");

  // The stack panel's help icon, right of its "Stack" title (not
  // observable; its place below the conflict banner and prompt box).
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
  const search = page.getByRole("textbox", { name: "Search files…" });
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
      "The workspace title sits under the prompt box as a large heading, wrapped rather than truncated",
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

test("code: Escape closes the prompt box's Add menu", async ({ page }) => {
  await home(page);
  const add = page.getByRole("button", { name: "Add", exact: true });
  const box = await add.boundingBox();
  if (!box) throw new Error("no Add button");
  // The menu's items are not observable elements: compare the area the
  // menu opens over instead.
  const under = { x: box.x, y: box.y + box.height, width: 200, height: 100 };
  const closed = await page.screenshot({ clip: under });
  await add.click();
  await settle(page, 600);
  expect(await page.screenshot({ clip: under }), "the Add menu opened").not.toEqual(closed);
  await capture(page, {
    name: "03-code-add-menu-open",
    title: "Prompt box: Add menu open",
    expectations: ["File and Attach listed under Add"],
  });
  await page.keyboard.press("Escape");
  await settle(page, 600);
  await capture(page, {
    name: "03-code-add-menu-escape",
    title: "Prompt box after Escape with the Add menu open",
    expectations: ["The Add menu is closed; the prompt box is unchanged"],
  });
  expect(await page.screenshot({ clip: under }), "Escape closed the Add menu").toEqual(closed);
});
