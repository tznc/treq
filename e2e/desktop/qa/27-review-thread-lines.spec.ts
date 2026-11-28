// GitHub review threads whose line is not in the Changes tab's diff sit
// above the file: each names its line ("Outdated · line N" for an outdated
// one), threads on one line sit together under one label, and Quote opens
// its comment box right under the thread it quotes. The fake `gh` puts an
// outdated thread and a current one on line 654 of cache.ts (a 9-line file),
// and one on line 700, through `$HOME/fake-gh-review-threads.json`.
import { rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { expect, test } from "@playwright/test";
import { fixture } from "../fixture.ts";
import { capture } from "./capture.ts";
import { openTab, openWorkspace, settle } from "./helpers.ts";

const overrides = () => join(fixture().home, "fake-gh-review-threads.json");
test.afterAll(() => rmSync(overrides(), { force: true }));

const PATH = "packages/api/src/cache.ts";

function thread(id: string, login: string, body: string, line: number | null, original: number) {
  return {
    id: `PRRT_${id}`,
    isResolved: false,
    isOutdated: line === null,
    path: PATH,
    line,
    originalLine: original,
    diffSide: "RIGHT",
    comments: {
      nodes: [
        {
          id: `PRRC_${id}`,
          body,
          createdAt: "2026-11-18T09:10:00Z",
          url: `https://github.com/o/r/pull/401#r_${id}`,
          diffHunk: "@@ -650,2 +650,3 @@\n+    Enum.each(deleted, &after_backend_deleted/1)",
          author: { login, avatarUrl: null },
        },
      ],
    },
  };
}

const THREADS = {
  401: [
    thread(
      "hook",
      "djwhitt",
      "> `Users.delete_user/1` calls this inside its transaction, but the hook stops ingestion. Run the lifecycle hooks via `Repo.after_commit/1` so they only run on success.",
      null,
      654,
    ),
    thread(
      "suggest",
      "zrzka",
      "```suggestion\n    Enum.each(deleted, fn b -> Repo.after_commit(fn -> after_backend_deleted(b) end) end)\n```",
      654,
      654,
    ),
    thread("later", "octocat", "Is this still needed?", 700, 700),
  ],
};

test("review-thread-lines: grouped by line, Quote under its thread", async ({ page }) => {
  writeFileSync(overrides(), JSON.stringify(THREADS));
  await openWorkspace(page, "feat/keyvalues-cache");
  await openTab(page, "Changes");
  await settle(page, 1500);
  await page.getByRole("button", { name: PATH, exact: true }).click();
  await settle(page, 2000);
  await expect(page.getByRole("group", { name: "Threads on line 654" })).toBeVisible();
  await expect(page.getByRole("group", { name: "Threads on line 700" })).toBeVisible();
  await capture(page, {
    name: "27-review-thread-lines-01-grouped",
    title: "Threads above the file, grouped by line",
    expectations: [
      "Above the diff, a muted 'Outdated · line 654' label heads djwhitt's and zrzka's threads, stacked together",
      "A separate 'Line 700' label heads octocat's thread below them",
      "Each thread keeps its '1 comment' pill",
    ],
  });

  await page.getByRole("button", { name: "Quote" }).first().click();
  const box = page.getByRole("textbox", { name: "Add a comment…" });
  await expect(box).toBeVisible();
  await settle(page, 800);
  await capture(page, {
    name: "27-review-thread-lines-02-quote",
    title: "Quote opens its box under the quoted thread",
    expectations: [
      "A comment box 'Quoting @djwhitt' sits directly under djwhitt's comment",
      "zrzka's thread follows below the comment box, not above it",
    ],
  });
});
