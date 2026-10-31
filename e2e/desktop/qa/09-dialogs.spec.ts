// Workspace dialogs: create (Ctrl+N) and its Advanced section, stack
// (Ctrl+Shift+N and the row's Stack button), rename (the row's context
// menu), delete (the palette's Delete Workspace and its confirmation), the
// agent prompt (Ctrl+I) with its agent and workspace pickers, and the
// toasts that report the result.
//
// The create test makes a scratch workspace and the delete test removes
// it again, so the sidebar is back to the seed for later areas.
import { expect, test, type Page } from "@playwright/test";
import { capture } from "./capture.ts";
import {
  dialogIn,
  focusMain,
  home,
  mod,
  openWorkspace,
  overlay,
  popupBox,
  ready,
  row,
  settle,
  xPointer,
} from "./helpers.ts";

test.describe.configure({ mode: "serial" });

const SCRATCH_TITLE = "QA scratch space";

const heading = (p: Page) => p.getByRole("heading", { name: "New workspace" });

test("dialogs: create workspace", async ({ page, context }) => {
  await home(page);
  await page.keyboard.press(`${mod}+n`);
  const dialog = await dialogIn(page, context, heading);
  await settle(page, 800);
  await capture(page, {
    name: "09-create-workspace-dialog",
    title: "New workspace dialog (Ctrl+N) from home",
    screen: true,
    rest: false,
    expectations: [
      "'Workspaces' textarea focused; Stack on reads the default branch",
      "Cancel and a disabled Create workspace at the bottom right",
      "Backdrop dims the window evenly",
    ],
  });
  // Opening Advanced only makes the left column taller: the heading keeps
  // its place.
  {
    const before = await heading(dialog).boundingBox();
    await dialog.getByRole("button", { name: "Advanced" }).click();
    await settle(page, 800);
    expect((await heading(dialog).boundingBox())?.x).toBe(before?.x);
    await capture(page, {
      name: "09-create-workspace-advanced",
      title: "New workspace: Advanced section open",
      screen: true,
      rest: false,
      expectations: [
        "Sparse paths and Symlink textareas side by side under Stack on",
        "Dialog keeps its size without clipping its buttons",
      ],
    });
  }
  await dialog.getByLabel("Workspaces", { exact: true }).fill(SCRATCH_TITLE);
  await settle(page, 1200); // the line check is debounced
  await dialog.getByRole("status").click();
  await settle(page, 500);
  await capture(page, {
    name: "09-create-workspace-title",
    title: "New workspace: branch named from the title",
    screen: true,
    rest: false,
    expectations: ["The popover lists prefix/qa-scratch-space as a new branch"],
  });
  await dialog.keyboard.press("Escape");
  await dialog.getByRole("button", { name: "Create workspace" }).click();
  await expect(heading(dialog)).toHaveCount(0, { timeout: 30_000 });
  await settle(page, 1500);
  await capture(page, {
    name: "09-create-workspace-created",
    title: "Workspace created: new sidebar row and toast",
    screen: true,
    rest: false,
    expectations: [
      "The new workspace shows in the sidebar and opens",
      "A success toast (if any) sits inside the window, not over the sidebar",
    ],
  });
});

test("dialogs: delete workspace", async ({ page, context }) => {
  await home(page);
  await page.keyboard.press(`${mod}+k`);
  const palette = await overlay(page, context);
  await palette.getByRole("textbox", { name: "Type a command or search…" }).fill("Delete workspace");
  await palette.getByRole("option", { name: "Delete workspace" }).click();
  const search = (p: Page) => p.getByRole("textbox", { name: "Search workspaces to delete…" });
  const dialog = await dialogIn(page, context, search);
  await settle(page, 800);
  await capture(page, {
    name: "09-delete-workspace-search",
    title: "Delete Workspace: pick the workspace",
    screen: true,
    rest: false,
    expectations: ["Workspaces listed with branch and repo; destructive intent clear"],
  });
  await search(dialog).fill("qa-scratch");
  await settle(page, 800);
  await capture(page, {
    name: "09-delete-workspace-filtered",
    title: "Delete Workspace: filtered to the scratch workspace",
    screen: true,
    rest: false,
    expectations: ["Only the matching workspace remains, highlighted"],
  });
  // Picking asks first, in the in-app confirmation: the question names
  // the workspace, and the buttons are Cancel and "Delete workspace".
  await dialog.keyboard.press("Enter").catch(() => {});
  const question = page.getByRole("dialog", { name: /^Delete .*qa-scratch.*\?$/ });
  const remove = page.getByTestId("confirm-confirm");
  await expect(question).toBeVisible();
  await expect(remove).toHaveAccessibleName("Delete workspace");
  await settle(page, 800);
  await capture(page, {
    name: "09-delete-workspace-confirm",
    title: "Delete Workspace: in-app confirmation",
    screen: true,
    rest: false,
    expectations: [
      "The title asks 'Delete <workspace>?' and the message says what is removed",
      "Cancel and a red 'Delete workspace' button, bottom right",
    ],
  });
  await remove.click();
  await page.waitForTimeout(2500);
  await capture(page, {
    name: "09-delete-workspace-deleted",
    title: "Workspace deleted: toast",
    screen: true,
    rest: false,
    expectations: [
      "'Workspace deleted' toast; the row left the sidebar",
      "The app lands on a sensible view (home), not an empty workspace",
    ],
  });
  await ready(page);
  await expect(page.getByRole("button", { name: /qa-scratch/ })).toHaveCount(0);
});

test("dialogs: stack workspace", async ({ page, context }) => {
  await openWorkspace(page, "feat/keyvalues-cache");
  await row(page, "feat/keyvalues-cache").hover();
  await row(page, "feat/keyvalues-cache").getByRole("button", { name: "Stack on this…" }).click();
  const dialog = await dialogIn(page, context, heading);
  await settle(page, 1200);
  await capture(page, {
    name: "09-stack-dialog",
    title: "Stack on feat/keyvalues-cache opens New workspace",
    screen: true,
    rest: false,
    expectations: [
      "'Stack on' reads feat/keyvalues-cache",
      "Buttons aligned at the bottom right",
    ],
  });
  await dialog.getByRole("button", { name: "Cancel" }).click();
  await expect(heading(dialog)).toHaveCount(0);

  // Ctrl+Shift+N stacks on the open workspace.
  await page.keyboard.press(`${mod}+Shift+N`);
  const again = await dialogIn(page, context, heading);
  await settle(page, 1000);
  await capture(page, {
    name: "09-stack-dialog-shortcut",
    title: "New workspace from Ctrl+Shift+N",
    screen: true,
    rest: false,
    expectations: ["Same dialog, 'Stack on' reads the open workspace"],
  });
  await again.getByRole("button", { name: "Cancel" }).click();
});

test("dialogs: rename workspace from the context menu", async ({ page, context }) => {
  await ready(page);
  await row(page, "feat/alerting-logs").click({ button: "right" });
  await settle(page, 1000);
  focusMain();
  const menu = popupBox();
  if (!menu) throw new Error("no context menu");
  // "Open in…" (the sixth row) opens a submenu on hover.
  await xPointer(page, menu.x + 100, menu.y + 190);
  await settle(page, 900);
  await capture(page, {
    name: "09-context-menu-submenu",
    title: "Workspace context menu: Open in… submenu",
    screen: true,
    rest: false,
    expectations: [
      "Submenu opens beside its row, top aligned with it",
      "Editors listed with icons; nothing clipped at the screen edge",
    ],
  });
  // "Rename workspace" is the third row.
  await xPointer(page, menu.x + 100, menu.y + 85, 1);
  const name = (p: Page) => p.getByRole("textbox", { name: "e.g., feat/new-name" });
  const dialog = await dialogIn(page, context, name);
  await settle(page, 800);
  await capture(page, {
    name: "09-rename-dialog",
    title: "Rename Workspace dialog",
    screen: true,
    rest: false,
    expectations: [
      "Branch Name prefilled with the current name and focused",
      "Rename disabled until the name changes",
    ],
  });
  await name(dialog).fill("feat/keyvalues-cache");
  await settle(page, 1000);
  await capture(page, {
    name: "09-rename-dialog-taken",
    title: "Rename Workspace: name already taken",
    screen: true,
    rest: false,
    expectations: ["An inline error says the branch exists; Rename disabled"],
  });
  await name(dialog).fill("feat/alerting logs!");
  await settle(page, 1000);
  await capture(page, {
    name: "09-rename-dialog-invalid",
    title: "Rename Workspace: invalid branch name",
    screen: true,
    rest: false,
    expectations: ["Invalid characters flagged before submit"],
  });
  await dialog.getByRole("button", { name: "Cancel" }).click();
  await expect(name(dialog)).toHaveCount(0);
});

test("dialogs: agent prompt (Ctrl+I) and its pickers", async ({ page, context }) => {
  await openWorkspace(page, "feat/keyvalues-cache");
  await page.keyboard.press(`${mod}+i`);
  const title = (p: Page) => p.getByRole("dialog", { name: "Start a new agent session" });
  const dialog = await dialogIn(page, context, title);
  await settle(page, 1000);
  // Opened from a workspace, the prompt targets it rather than main.
  await expect(
    dialog.getByRole("button", { name: "Workspace: feat/keyvalues-cache" }),
  ).toBeVisible();
  await capture(page, {
    name: "09-agent-prompt",
    title: "Start a new agent session (Ctrl+I)",
    screen: true,
    rest: false,
    expectations: [
      "Prompt field focused; workspace and agent pickers in the footer",
      "The workspace picker shows feat/keyvalues-cache, the open workspace",
      "A Send icon button ends the footer; no Edit/Plan split menu",
    ],
  });
  await dialog.getByRole("textbox", { name: "Describe a task…" }).last().fill(
    "Add a TTL to the key-value cache and cover expiry in tests",
  );
  await settle(page, 500);
  await capture(page, {
    name: "09-agent-prompt-typed",
    title: "Agent prompt with a task typed",
    screen: true,
    rest: false,
    expectations: ["Text wraps in the field; the Send button is enabled"],
  });
  // The agent select (Claude) beside Send, at its place in the dialog.
  {
    await page.mouse.click(829, 526);
    await settle(page, 900);
    await capture(page, {
      name: "09-agent-prompt-agent-picker",
      title: "Agent prompt: agent picker",
      screen: true,
      rest: false,
      expectations: [
        "Agents listed with icons; the current one marked",
        "Agents whose CLI isn't installed are not listed",
      ],
    });
    await dialog.keyboard.press("Escape");
    await settle(page, 500);
  }
  await dialog.getByRole("button", { name: /^Workspace: / }).click();
  await settle(page, 900);
  await capture(page, {
    name: "09-agent-prompt-workspace-picker",
    title: "Agent prompt: workspace picker",
    screen: true,
    rest: false,
    expectations: ["Workspaces searchable; 'new workspace' option distinct"],
  });
  // Escape closes the picker and hands the keyboard back to the prompt, so
  // a second Escape closes the dialog.
  await dialog.keyboard.press("Escape");
  await settle(page, 400);
  await capture(page, {
    name: "09-agent-prompt-picker-escaped",
    title: "Agent prompt after Escape in the workspace picker",
    screen: true,
    rest: false,
    expectations: [
      "The workspace picker is closed and the dialog is still open",
      "The prompt field has focus again",
    ],
  });
  await dialog.keyboard.press("Escape");
  await settle(page, 600);
  await capture(page, {
    name: "09-agent-prompt-second-escape",
    title: "A second Escape closes the agent prompt",
    screen: true,
    rest: false,
    expectations: ["The agent prompt dialog is closed"],
  });
  await expect(title(dialog)).toHaveCount(0);
});
