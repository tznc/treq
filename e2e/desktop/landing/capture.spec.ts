// Captures the landing page screenshots from the running app on the demo
// repository (`seed.ts`) and writes each PNG named in
// web/src/data/landing-screenshots.json into web/static/img/landing/ (or
// `LANDING_OUT`). The steps share one app and run in order.
//
// A few targets (file rows in the Changes list, a diff line's comment
// button, a thread's toggle, the GitHub tabs) are not observable elements,
// so those steps click a fixed point. The window is a fixed 1400x900 and the
// seed is deterministic, so the points hold; each step then waits on an
// observable result, so a moved target fails the run instead of capturing
// the wrong state.
import { mkdirSync, readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { expect, type Page, test } from "@playwright/test";
import { fixture } from "../fixture.ts";
import {
  CACHE,
  CACHE_EDIT,
  FEED_SRC,
  MARKETING,
  writeInWorkspace,
} from "./seed.ts";

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(here, "../../..");
const outDir = resolve(
  process.env.LANDING_OUT ?? join(repoRoot, "web/static/img/landing"),
);
const expected: string[] = JSON.parse(
  readFileSync(join(repoRoot, "web/src/data/landing-screenshots.json"), "utf8"),
);
const written = new Set<string>();

type Clip = { x: number; y: number; width: number; height: number };

// Regions of the 1400x900 window.
const SIDEBAR: Clip = { x: 0, y: 0, width: 280, height: 900 };
const MAIN: Clip = { x: 280, y: 0, width: 1120, height: 900 };
/** The Changes tab: its file list and the diff, below the tab bar. */
const CHANGES: Clip = { x: 280, y: 102, width: 1120, height: 759 };

test.describe.configure({ mode: "serial" });

/**
 * Moves the pointer to the empty foot of the sidebar, off every control and
 * list row. A hover tooltip only closes when the pointer travels away from
 * its target, so the move takes steps.
 */
async function rest(page: Page): Promise<void> {
  await page.mouse.move(140, 880, { steps: 12 });
  await page.waitForTimeout(400);
}

async function shot(page: Page, name: string, clip?: Clip): Promise<void> {
  expect(expected, `${name} is not in landing-screenshots.json`).toContain(
    name,
  );
  await rest(page);
  await page.screenshot({ path: join(outDir, name), clip });
  written.add(name);
}

/** The prompt card around the "Describe a task..." box. */
async function promptCard(page: Page): Promise<Clip> {
  const box = await page
    .getByRole("textbox", { name: "Describe a task..." })
    .boundingBox();
  const edit = await page
    .getByRole("button", { name: "Edit", exact: true })
    .boundingBox();
  if (!box || !edit) throw new Error("prompt card not on screen");
  const top = box.y - 15;
  return {
    x: box.x - 15,
    y: top,
    width: box.width + 30,
    height: edit.y + edit.height + 11 - top,
  };
}

function row(page: Page, branch: string) {
  return page.getByRole("button", { name: branch, exact: true });
}

async function openWorkspace(page: Page, branch: string): Promise<void> {
  await row(page, branch).click();
  await expect(
    page.getByRole("button", { name: branch, exact: true, selected: true }),
  ).toBeVisible();
}

async function openTab(page: Page, name: string): Promise<void> {
  await page.getByRole("tab", { name }).click();
  await expect(page.getByRole("tab", { name, selected: true })).toBeVisible();
  await page.waitForTimeout(1500);
}

/** Comments on a diff line through its gutter's add button. */
async function commentOnLine(
  page: Page,
  path: string,
  line: number,
  text: string,
): Promise<void> {
  const add = page.getByRole("button", {
    name: `Add comment on ${path} line ${line}`,
    // Without exact, "line 1" also matches "line 10" and "line 11".
    exact: true,
  });
  // The button only shows while the line is hovered, so the pointer goes there
  // first.
  const box = await add.boundingBox();
  if (!box) throw new Error(`no add comment button for ${path} line ${line}`);
  const [x, y] = [box.x + box.width / 2, box.y + box.height / 2];
  await page.mouse.move(x + 150, y);
  await page.mouse.move(x, y, { steps: 4 });
  await page.mouse.click(x, y);
  const input = page.getByRole("textbox", { name: "Add a comment..." });
  await expect(input).toBeVisible();
  await input.fill(text);
  await page.getByRole("button", { name: "Add Comment", exact: true }).click();
  await expect(input).toHaveCount(0);
}

async function typePrompt(
  page: Page,
  name: string,
  text: string,
): Promise<void> {
  const box = page.getByRole("textbox", { name: "Describe a task..." });
  await box.fill(text);
  await expect(box).toHaveValue(text);
  await shot(page, name, await promptCard(page));
}

test.beforeAll(() => {
  mkdirSync(outDir, { recursive: true });
});

test("captures the landing screenshots", async ({ page }) => {
  await page.setViewportSize({ width: 1400, height: 900 });
  await expect(
    page.getByRole("button", { name: "Command palette" }),
  ).toBeVisible({
    timeout: 30_000,
  });
  await expect(row(page, MARKETING)).toBeVisible({ timeout: 30_000 });
  // The PR status poll fills the stack's PR and CI state.
  await page.waitForTimeout(3000);

  // The sidebar's stacked workspaces, on the home view.
  await shot(page, "sidebar.png", SIDEBAR);

  // GitHub: issues, then pull requests (both from the fake `gh`).
  await page.getByRole("button", { name: "Github", exact: true }).click();
  await expect(
    page.getByRole("button", { name: "New", exact: true }),
  ).toBeVisible();
  await page.waitForTimeout(2000);
  await shot(page, "github-issues.png", MAIN);
  await page.mouse.click(422, 62); // the "Pull Requests" tab
  await page.waitForTimeout(2000);
  await shot(page, "github.png", MAIN);

  // Prompts typed into a workspace's task box.
  await openWorkspace(page, MARKETING);
  await openTab(page, "Code");
  await typePrompt(page, "prompt.png", "create a workspace for this fix");
  await typePrompt(
    page,
    "move-changes.png",
    "Move my changes to a new workspace and continue the fix there",
  );
  await typePrompt(
    page,
    "break-workspace.png",
    "Break up this workspace into smaller stacked or separate workspaces",
  );
  await page.getByRole("textbox", { name: "Describe a task..." }).fill("");
  await openWorkspace(page, CACHE);
  await openTab(page, "Code");
  await typePrompt(
    page,
    "spawn-before.png",
    "Split the work across 3 agents in 3 different workspaces",
  );
  await page.getByRole("textbox", { name: "Describe a task..." }).fill("");

  // The conflicted Home.tsx, handed to an agent for a new commit.
  await openWorkspace(page, MARKETING);
  await openTab(page, "Changes");
  const resolve = page.getByRole("button", { name: "Resolve conflicts..." });
  await expect(resolve).toBeVisible();
  await page.mouse.click(330, 283); // Home.tsx under Conflicts
  await page.waitForTimeout(1000);
  await resolve.click();
  await expect(page.getByRole("button", { name: "Plan" })).toBeVisible();
  await shot(page, "conflict-new-commit.png", CHANGES);
  await page.mouse.click(1362, 171); // the popover's close button
  await expect(page.getByRole("button", { name: "Plan" })).toHaveCount(0);

  // A resolved GitHub thread and a local comment on client.ts.
  await page.mouse.click(326, 383); // client.ts under Committed
  await page.waitForTimeout(1500);
  await commentOnLine(page, "packages/web/src/lib/client.ts", 6, "remove this");
  await page.mouse.click(571, 474); // unfold the resolved thread
  await page.waitForTimeout(800);
  await shot(page, "review.png", CHANGES);

  // The conflicted commit, resolved in place from the Commits tab.
  await openTab(page, "Commits");
  await page.getByRole("button", { name: "Resolve conflicts…" }).click();
  const instructions = page.getByRole("textbox", {
    name: /^e\.g\. Prefer the workspace-side changes/,
  });
  await expect(instructions).toBeVisible();
  const field = await instructions.boundingBox();
  const action = await page
    .getByRole("button", { name: "Resolve", exact: true })
    .boundingBox();
  if (!field || !action) throw new Error("resolve dialog not on screen");
  await shot(page, "conflict-inplace.png", {
    x: field.x - 24,
    y: field.y - 56,
    width: field.width + 48,
    height: action.y + action.height + 24 - (field.y - 56),
  });
  await page.getByRole("button", { name: "Cancel" }).click();

  // Two workspaces, each with its own uncommitted edit.
  const { repo } = fixture();
  writeInWorkspace(repo, CACHE, "packages/api/src/cache.ts", CACHE_EDIT);
  writeInWorkspace(repo, MARKETING, "packages/web/src/lib/feed.ts", FEED_SRC);
  await openWorkspace(page, CACHE);
  await openTab(page, "Changes");
  await shot(page, "isolation-left.png", CHANGES);
  await openWorkspace(page, MARKETING);
  await openTab(page, "Changes");
  await shot(page, "isolation-right.png", CHANGES);

  // Review comments on an uncommitted and a committed file.
  await openWorkspace(page, CACHE);
  await openTab(page, "Changes");
  await commentOnLine(page, "packages/api/src/cache.test.ts", 4, "cover the miss path");
  await commentOnLine(page, "packages/api/src/cache.ts", 1, "keep the map private");
  await shot(page, "code-reviews.png", CHANGES);

  // Three agent workspaces stacked on the cache workspace.
  for (const name of ["one", "two", "three"]) {
    const branch = `feat/agent-${name}`;
    await row(page, CACHE).hover();
    await row(page, CACHE)
      .getByRole("button", { name: "Stack on this…" })
      .click();
    await page.getByLabel("Workspaces", { exact: true }).fill(branch);
    await page.getByRole("button", { name: "Create workspace" }).click({ timeout: 20_000 });
    await expect(row(page, branch)).toBeVisible({ timeout: 20_000 });
    await rest(page);
  }
  await openWorkspace(page, "feat/agent-one");
  await page.waitForTimeout(1500);
  await shot(page, "spawn-after.png", SIDEBAR);

  // `treq send` from the workspace shell: the thumbnail, then the lightbox.
  await openWorkspace(page, MARKETING);
  await openTab(page, "Commits");
  await row(page, MARKETING).hover();
  await row(page, MARKETING)
    .getByRole("button", { name: "Open shell" })
    .click();
  await page.waitForTimeout(2500);
  await page.mouse.click(900, 760); // focus the terminal
  // A short prompt, and no "Sent <absolute path>" line, keep the fixture's
  // temp dir out of the shot.
  await page.keyboard.type("PS1='event-bus $ '; clear\n");
  await page.waitForTimeout(800);
  await page.keyboard.type("treq send ~/research-draft.svg >/dev/null\n");
  await page.waitForTimeout(3000);
  await shot(page, "send-thumb.png", {
    x: 280,
    y: 603,
    width: 1120,
    height: 297,
  });
  await page.mouse.click(364, 690); // the thumbnail
  await page.waitForTimeout(1500);
  await shot(page, "send-lightbox.png");
  await page.keyboard.press("Escape");
  await page.waitForTimeout(800);

  // The Commits tab with a commit open and an agent in the terminal pane.
  await row(page, MARKETING).hover();
  await row(page, MARKETING)
    .getByRole("button", { name: "Start agent" })
    .click();
  await page.waitForTimeout(3000);
  await page.mouse.click(330, 346); // unfold "feat: handle empty event messages"
  await page.waitForTimeout(1500);
  await shot(page, "commits.png");

  // Every PNG the site lists came from this run.
  expect([...written].sort()).toEqual([...expected].sort());
});
