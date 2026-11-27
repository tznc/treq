// Text in review comments selects with a drag: a GitHub comment's, after
// which Quote quotes only the selection, and a local review comment's (a
// review sent to an agent leaves one on the diff). The fake `gh` puts a
// thread on cache.ts through `$HOME/fake-gh-review-threads.json`.
import { rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { expect, test, type Page } from "@playwright/test";
import { fixture } from "../fixture.ts";
import { capture } from "./capture.ts";
import { openTab, openWorkspace, settle } from "./helpers.ts";

const overrides = () => join(fixture().home, "fake-gh-review-threads.json");
test.afterAll(() => rmSync(overrides(), { force: true }));

const THREADS = {
  401: [
    {
      id: "PRRT_hook",
      isResolved: false,
      isOutdated: false,
      path: "packages/api/src/cache.ts",
      line: 3,
      diffSide: "RIGHT",
      comments: {
        nodes: [
          {
            id: "PRRC_hook",
            body: "This calls the hook inside its transaction, so a rollback cannot undo it.",
            createdAt: "2026-11-18T09:10:00Z",
            url: "https://github.com/o/r/pull/401#r_hook",
            diffHunk: "@@ -0,0 +1,3 @@\n+const store = new Map<string, string>();\n+\n+export function setKey(key: string, value: string): void {",
            author: { login: "djwhitt", avatarUrl: null },
          },
        ],
      },
    },
  ],
};

/** Drags across `width` pixels of the line at `y`, from `x`. */
async function drag(page: Page, x: number, y: number, width: number): Promise<void> {
  await page.mouse.move(x, y);
  await page.mouse.down();
  await page.mouse.move(x + width, y, { steps: 6 });
  await page.mouse.up();
  await settle(page, 400);
}

test("review-comment-selection: quote the selected text", async ({ page }) => {
  writeFileSync(overrides(), JSON.stringify(THREADS));
  await openWorkspace(page, "feat/keyvalues-cache");
  await openTab(page, "Changes");
  await settle(page, 1500);
  await page.getByRole("button", { name: "packages/api/src/cache.ts", exact: true }).click();
  await settle(page, 2000);
  const text = page.getByText("calls the hook inside its transaction", { exact: false }).first();
  const box = await text.boundingBox();
  expect(box).not.toBeNull();
  if (!box) return;
  await drag(page, box.x + 2, box.y + box.height / 2, 200);
  await capture(page, {
    name: "28-review-comment-selection-01-github",
    title: "A GitHub comment's text, selected",
    expectations: [
      "The start of djwhitt's comment ('This calls the hook…') is highlighted as a text selection",
    ],
  });
  await page.getByRole("button", { name: "Quote" }).first().click();
  await expect(page.getByRole("textbox", { name: "Add a comment…" })).toBeVisible();
  await settle(page, 800);
  await capture(page, {
    name: "28-review-comment-selection-02-quoted",
    title: "Quote quotes only the selection",
    expectations: [
      "Under the thread, a comment box reads 'Quoting @djwhitt' over only the selected words, not the whole comment",
    ],
  });
});

test("review-comment-selection: a local review comment's text", async ({ page }) => {
  await openWorkspace(page, "feat/alerting-logs");
  await openTab(page, "Changes");
  await settle(page, 1500);
  // The first added line's gutter (by place in the 1400x900 window).
  await page.mouse.click(600, 277);
  await page.keyboard.press("ArrowLeft");
  await page.getByRole("textbox", { name: "Add a comment…" }).fill("Log the level in upper case");
  await page.getByRole("button", { name: "Add comment", exact: true }).click();
  await page.getByRole("button", { name: /^Finish review/ }).click();
  await page.getByRole("button", { name: "Plan", exact: true }).click();
  await expect(page.getByRole("button", { name: "Resolve", exact: true })).toBeVisible({
    timeout: 15_000,
  });
  await settle(page, 800);
  const card = await page.getByRole("group", { name: /^Sent comment on/ }).first().boundingBox();
  expect(card).not.toBeNull();
  if (!card) return;
  // The comment's text sits under the card's header row.
  await drag(page, card.x + 14, card.y + 50, 180);
  await capture(page, {
    name: "28-review-comment-selection-03-local",
    title: "A local review comment's text, selected",
    expectations: [
      "In the 'Sent to agent' card, the words 'Log the level in upper case' are highlighted as a text selection",
    ],
  });
});
