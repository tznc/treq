// "Show more lines" on a committed file: the gap between two hunks fills
// with the hidden lines, numbered on both sides. A committed file with no
// uncommitted edits reads the same in the working copy as at the branch
// head, so the lines come from there.
//
// The context rows are not observable, so the expander is clicked at a
// fixed point (for this scenario and the 1400x900 window).
import { execFileSync } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { test } from "@playwright/test";
import { appBinary } from "../env.ts";
import { fixture, git } from "../fixture.ts";
import { appEnv } from "../harness.ts";
import { writeInWorkspace } from "../landing/seed.ts";
import { capture } from "./capture.ts";
import { openTab, openWorkspace, settle } from "./helpers.ts";

const BRANCH = "feat/diff-context";
const FILE = "packages/api/src/routes.ts";
// The first hunk's "Show more lines below" and the second hunk's header.
const BELOW = { x: 1029, y: 483 };
const ABOVE = { x: 1029, y: 505 };

const lines = (edit: (n: number) => string) =>
  Array.from({ length: 60 }, (_, i) => edit(i + 1)).join("\n") + "\n";
const MAIN = lines((n) => `export const route${n} = "/v1/route-${n}";`);
// Old line 10 changes and gains two lines after it, and old line 40
// changes: old 14-36 / new 16-38 are hidden between the hunks.
const BRANCH_SRC = lines((n) =>
  n === 10
    ? `export const route10 = "/v2/route-10";\n// added\n// added`
    : n === 40
      ? `export const route40 = "/v2/route-40";`
      : `export const route${n} = "/v1/route-${n}";`,
);

test("diff context: a committed file's gap expands between hunks", async ({ page }) => {
  const f = fixture();
  const treq = (...args: string[]) =>
    execFileSync(appBinary, args, {
      cwd: f.repo,
      env: {
        ...appEnv({ ...f, bin: join(f.root, "bin") }),
        TREQ_APP_DATA_DIR: join(f.home, ".local/share/com.treq"),
      },
      stdio: "pipe",
    });
  mkdirSync(dirname(join(f.repo, FILE)), { recursive: true });
  writeFileSync(join(f.repo, FILE), MAIN);
  git("add", FILE);
  git("commit", "-m", "feat: list routes");
  treq("add", BRANCH);
  writeInWorkspace(f.repo, BRANCH, FILE, BRANCH_SRC);
  treq("commit", BRANCH, "-m", "feat: v2 route and comments");

  await openWorkspace(page, BRANCH);
  await openTab(page, "Changes");
  await settle(page, 1500);
  await capture(page, {
    name: "14-diff-context-03-committed-gap",
    title: "routes.ts committed: two hunks with a gap between them",
    expectations: [
      "A 'Show more lines below' row sits under the first hunk",
      "The second hunk's header follows it, with lines hidden in between",
    ],
  });
  // Above first: the lines land under the "below" row, which stays put.
  await page.mouse.click(ABOVE.x, ABOVE.y);
  await settle(page, 1000);
  await capture(page, {
    name: "14-diff-context-04-committed-above",
    title: "routes.ts after the second hunk's 'Show more lines above'",
    expectations: [
      "Twenty lines sit above the second hunk, numbered old 17-36 and new 19-38",
      "The lines are syntax highlighted like the hunk's context lines",
      "'Show more lines below' still sits under the first hunk: three lines stay hidden",
    ],
  });
  await page.mouse.click(BELOW.x, BELOW.y);
  await settle(page, 1000);
  await capture(page, {
    name: "14-diff-context-05-committed-closed",
    title: "routes.ts after the gap is fully expanded",
    expectations: [
      "Old 14-16 / new 16-18 follow the first hunk and run into old 17 / new 19",
      "No 'Show more lines' row or second hunk header remains between the hunks",
    ],
  });
});
