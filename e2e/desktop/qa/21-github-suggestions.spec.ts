// GitHub review comments with a ```suggestion block: drawn as a diff
// against the commented line, not as a code block. The fake `gh` puts a
// rename on cache.ts line 1 and an empty (delete-the-line) suggestion on
// line 4 of PR #401, through `$HOME/fake-gh-review-threads.json` so the
// shared fixture (and the landing captures) stay unchanged.
import { rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { expect, test } from "@playwright/test";
import { fixture } from "../fixture.ts";
import { capture } from "./capture.ts";
import { openTab, openWorkspace, settle } from "./helpers.ts";

const overrides = () => join(fixture().home, "fake-gh-review-threads.json");
test.afterAll(() => rmSync(overrides(), { force: true }));

const THREADS = {
  401: [
    {
      id: "PRRT_cache_rename",
      isResolved: false,
      isOutdated: false,
      path: "packages/api/src/cache.ts",
      line: 1,
      diffSide: "RIGHT",
      comments: {
        nodes: [
          {
            id: "PRRC_cache_rename",
            body: "Name it for what it holds.\n\n```suggestion\nconst values = new Map<string, string>();\n```",
            createdAt: "2026-08-14T09:10:00Z",
            url: "https://github.com/o/r/pull/401#r3",
            diffHunk: "@@ -0,0 +1 @@\n+const store = new Map<string, string>();",
            author: { login: "octocat", avatarUrl: null },
          },
        ],
      },
    },
    {
      id: "PRRT_cache_delete",
      isResolved: false,
      isOutdated: false,
      path: "packages/api/src/cache.ts",
      line: 4,
      diffSide: "RIGHT",
      comments: {
        nodes: [
          {
            id: "PRRC_cache_delete",
            body: "```suggestion\n```",
            createdAt: "2026-08-14T09:12:00Z",
            url: "https://github.com/o/r/pull/401#r4",
            diffHunk: "@@ -0,0 +1,4 @@\n+const store = new Map<string, string>();\n+\n+export function setKey(key: string, value: string): void {\n+  store.set(key, value);",
            author: { login: "hubot", avatarUrl: null },
          },
        ],
      },
    },
  ]
};

test("github-suggestions: replacement and deletion", async ({ page }) => {
  writeFileSync(overrides(), JSON.stringify(THREADS));
  await openWorkspace(page, "feat/keyvalues-cache");
  await openTab(page, "Changes");
  await settle(page, 1500);
  await page.getByRole("button", { name: "packages/api/src/cache.ts", exact: true }).click();
  await settle(page, 2000);
  await expect(page.getByRole("heading", { name: "Suggested change" }).first()).toBeVisible();
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
