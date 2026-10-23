// PR review drafts: an AI review's comment lands while the screen is open,
// you draft your own, and a push to the PR moves your line (the draft
// follows it) and changes the agent's line (its draft goes outdated and
// shows the code it was written on). The fake `gh` serves PR #418's diff
// from pr-diff-418.diff, rewritten here mid-run.
import { execFileSync } from "node:child_process";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { expect, test, type Page } from "@playwright/test";
import { appEnv } from "../harness.ts";
import { appBinary } from "../env.ts";
import { fixture } from "../fixture.ts";
import { capture } from "./capture.ts";
import { ready, settle } from "./helpers.ts";

const PRS_TAB = { x: 422, y: 62 };
const FIRST_ROW = { x: 700, y: 147 };
const FILE = "packages/web/src/lib/search.ts";

const diff = (lines: string[]) => `diff --git a/${FILE} b/${FILE}
index 1111111..2222222 100644
--- a/${FILE}
+++ b/${FILE}
@@ -1,6 +1,${lines.length} @@
${lines.join("\n")}
`;

const BEFORE = diff([
  ' import { index } from "./index";',
  " ",
  " export function search(q: string) {",
  "-  return index.filter((item) => item.name.includes(q));",
  "+  const query = q.trim().toLowerCase();",
  "+  if (!query) return [];",
  "+  return index.filter((item) => item.name.includes(q));",
  " }",
]);

// Pushed later: a comment above `const query` moves it down a line, and
// the filter line the agent commented on is rewritten.
const AFTER = diff([
  ' import { index } from "./index";',
  " ",
  " export function search(q: string) {",
  "-  return index.filter((item) => item.name.includes(q));",
  "+  // Normalise before matching.",
  "+  const query = q.trim().toLowerCase();",
  "+  if (!query) return [];",
  "+  return index.filter((item) => item.name.startsWith(query));",
  " }",
]);

function setDiff(text: string): void {
  writeFileSync(join(fixture().root, "pr-diff-418.diff"), text);
}

/** `treq agent-review add`, as the AI review agent runs it. */
function agentComment(args: string[]): void {
  const f = fixture();
  execFileSync(appBinary, ["agent-review", "add", "--target-type", "github_pr", "--target-id", "418", ...args], {
    cwd: f.repo,
    env: {
      ...appEnv({ ...f, bin: join(f.root, "bin") }),
      TREQ_APP_DATA_DIR: join(f.home, ".local/share/com.treq"),
    },
    stdio: "pipe",
  });
}

async function openReview(page: Page): Promise<void> {
  await page.getByRole("button", { name: "Review changes" }).click();
  await settle(page, 3000);
}

test("pr review drafts: agent draft, your draft, moved and outdated after a push", async ({ page }) => {
  setDiff(BEFORE);
  await ready(page);
  await page.getByRole("button", { name: "Github", exact: true }).click();
  await settle(page, 2000);
  await page.mouse.click(PRS_TAB.x, PRS_TAB.y);
  await settle(page, 1500);
  await page.mouse.click(FIRST_ROW.x, FIRST_ROW.y);
  await settle(page, 2500);
  await openReview(page);
  await capture(page, {
    name: "19-pr-review-01-diff",
    title: "PR #418 review screen with its diff",
    expectations: ["search.ts diff with the trimmed query lines; no drafts yet"],
  });

  // The AI review records a comment with a suggestion; the screen polls.
  agentComment([
    "--file", FILE, "--start-line", "6", "--end-line", "6",
    "--comment", "The filter still matches on `q`, so the trimmed `query` is never used.",
    "--suggestion", "  return index.filter((item) => item.name.toLowerCase().includes(query));",
  ]);
  await expect(page.getByRole("button", { name: "Finish review (1)" })).toBeVisible({ timeout: 10_000 });
  await settle(page, 800);
  await capture(page, {
    name: "19-pr-review-02-agent-draft",
    title: "The agent's comment appears as a draft without navigating",
    expectations: ["A draft with a suggestion under line 6", "Header reads Finish review (1)"],
  });

  // Your own draft on `const query` (line 4): the gutter, then Left.
  // Not observable, so by its place in the 1400x900 window.
  await page.mouse.click(600, 243);
  await page.keyboard.press("ArrowLeft");
  const input = page.getByRole("textbox", { name: "Add a comment..." });
  await expect(input).toBeVisible();
  await input.fill("Lower-casing here changes matching for mixed-case names.");
  await page.getByRole("button", { name: "Add Comment", exact: true }).click();
  await expect(page.getByRole("button", { name: "Finish review (2)" })).toBeVisible({ timeout: 10_000 });
  await settle(page, 800);
  await capture(page, {
    name: "19-pr-review-03-two-drafts",
    title: "Your draft and the agent's",
    expectations: ["Your draft under line 4 and the agent's under line 6", "Finish review (2)"],
  });

  // Leave and come back: the drafts are stored, not in memory.
  await page.getByRole("button", { name: "Back" }).click();
  await settle(page, 1500);
  setDiff(AFTER); // the author pushes meanwhile
  await openReview(page);
  await expect(page.getByRole("button", { name: "Finish review (1)" })).toBeVisible({ timeout: 10_000 });
  await settle(page, 800);
  await capture(page, {
    name: "19-pr-review-04-after-push",
    title: "After a push: your draft followed its line, the agent's is outdated",
    expectations: [
      "Your draft sits under `const query` at its new line 5",
      "An amber Outdated box shows the agent's draft with the original `includes(q)` line",
      "Finish review (1): only your draft will be posted",
    ],
  });

  await page.getByRole("button", { name: "Finish review (1)" }).click();
  await settle(page, 1000);
  // octocat opened #418 and is the signed-in user. The options are not
  // observable, so the capture checks them.
  await capture(page, {
    name: "19-pr-review-05-finish",
    title: "Finish your review",
    expectations: [
      "1 pending comment; 1 outdated draft stays as a draft",
      "Approve and Request changes disabled on your own PR",
    ],
    screen: true,
  });
});
