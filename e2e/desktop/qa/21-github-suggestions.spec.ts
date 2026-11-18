// GitHub review comments with a ```suggestion block: drawn as a diff
// against the commented line, not as a code block. The fake `gh` puts a
// rename on cache.ts line 1 and an empty (delete-the-line) suggestion on
// line 4 of PR #401.
import { expect, test } from "@playwright/test";
import { capture } from "./capture.ts";
import { openTab, openWorkspace, settle } from "./helpers.ts";

test("github-suggestions: replacement and deletion", async ({ page }) => {
  await openWorkspace(page, "feat/keyvalues-cache");
  await openTab(page, "Changes");
  await settle(page, 1500);
  await page.getByRole("button", { name: "packages/api/src/cache.ts", exact: true }).click();
  await settle(page, 2000);
  await expect(page.getByRole("heading", { name: "Suggested change" })).toHaveCount(2);
  await capture(page, {
    name: "21-github-suggestions-01-diffs",
    title: "GitHub suggestions drawn as diffs",
    expectations: [
      "Line 1's thread: 'Name it for what it holds.' then a Suggested change box with '- const store' and '+ const values', changed words highlighted",
      "Line 4's thread: a Suggested change box with one red '- store.set(key, value);' row, not an empty box",
      "No raw ```suggestion fence or empty code block is visible",
    ],
  });
});
