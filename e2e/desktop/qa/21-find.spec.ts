// The shared Cmd+F find bar: the Commits tab (headlines and open diffs),
// the Changes diff and an open file in the Code tab.
import { expect, test } from "@playwright/test";
import { capture } from "./capture.ts";
import { mod, openTab, openWorkspace, settle } from "./helpers.ts";

test.describe.configure({ mode: "serial" });

const BRANCH = "feat/empty-event-message";
/** A point on a commit row's headline: an unfolded row also holds its diff. */
const TITLE = { x: 80, y: 12 };

test("find: commits headlines and an open diff", async ({ page }) => {
  await openWorkspace(page, BRANCH);
  await openTab(page, "Commits");
  await settle(page, 1500);
  await page.mouse.click(700, 600);
  await page.keyboard.press(`${mod}+f`);
  await settle(page, 600);
  await page.keyboard.type("event");
  await settle(page, 800);
  await capture(page, {
    name: "21-find-commits-headlines",
    title: "Commits: find in commit headlines",
    expectations: [
      "Find bar top right with 'event', a '1 of N' count and prev/next/close",
      "'event' highlighted in the commit headlines; the first match stronger",
    ],
  });

  // Unfold the top commit: its diff joins the matches.
  const commit = page.getByRole("button", { name: "feat: wire event feed on the home page", exact: true });
  if ((await page.getByRole("button", { name: "Edit description" }).count()) === 0) {
    await commit.click({ position: TITLE });
  }
  await settle(page, 2000);
  await capture(page, {
    name: "21-find-commits-diff",
    title: "Commits: find covers an unfolded diff",
    expectations: [
      "The unfolded commit's diff lines mark 'event' too",
      "The count's total grew from the headlines-only run",
    ],
  });

  for (let i = 0; i < 3; i++) await page.getByRole("button", { name: "Next match" }).click();
  await settle(page, 800);
  await capture(page, {
    name: "21-find-commits-step",
    title: "Commits: stepping with Next",
    expectations: ["Count reads '4 of N'; the current (stronger) mark moved down to a later match"],
  });
  await page.keyboard.press("Escape");
  await page.getByRole("button", { name: "Close search" }).click().catch(() => {});
  await settle(page, 500);
  await expect(page.getByRole("button", { name: "Next match" })).toHaveCount(0);
});

test("find: changes diff", async ({ page }) => {
  await openWorkspace(page, BRANCH);
  await openTab(page, "Changes");
  await page.mouse.click(900, 500);
  await page.keyboard.press(`${mod}+f`);
  await settle(page, 600);
  await page.keyboard.type("event");
  await settle(page, 1000);
  await page.keyboard.press("Enter");
  await settle(page, 800);
  await capture(page, {
    name: "21-find-changes",
    title: "Changes: the same find bar in the diff",
    expectations: [
      "Same bar as Commits: query, count '2 of N' after Enter, prev/next/close",
      "Matches marked in the diff; the current one stronger",
    ],
  });
  await page.keyboard.press("Escape");
});

test("find: open file in the Code tab", async ({ page }) => {
  await openWorkspace(page, BRANCH);
  await openTab(page, "Code");
  await page.keyboard.press(`${mod}+p`);
  const picker = page.getByRole("textbox", { name: "Go to file…" });
  await expect(picker).toBeVisible();
  await picker.fill("README");
  await settle(page, 800);
  await page.keyboard.press("Enter");
  await settle(page, 1500);
  await page.mouse.click(900, 400);
  await page.keyboard.press(`${mod}+f`);
  await settle(page, 600);
  await page.keyboard.type("the");
  await settle(page, 1000);
  await capture(page, {
    name: "21-find-file",
    title: "Code tab: the same find bar in an open file",
    expectations: [
      "Same bar top right of the file, with a '1 of N' count",
      "'the' highlighted in the file's lines; the first match stronger",
    ],
  });
  await page.keyboard.press("Escape");
});
