// Comment card width: on a wide window, a long agent comment on PR #418
// stays a readable width instead of spanning the whole diff.
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

test("comment card width: long agent comment on a wide window", async ({ page }) => {
  setDiff(BEFORE);
  await ready(page);
  await page.setViewportSize({ width: 2000, height: 900 });
  await settle(page, 800);
  await page.getByRole("button", { name: "GitHub", exact: true }).click();
  await settle(page, 2000);
  await page.mouse.click(PRS_TAB.x, PRS_TAB.y);
  await settle(page, 1500);
  await page.mouse.click(FIRST_ROW.x, FIRST_ROW.y);
  await settle(page, 2500);
  await openReview(page);
  agentComment([
    "--file", FILE, "--start-line", "6", "--end-line", "6",
    "--comment",
    "The filter still matches on `q`, so the trimmed and lowercased `query` computed above is never used. " +
      "Searches with leading or trailing whitespace, or different casing, return nothing even though a match exists. " +
      "Consider matching on `query` and lowercasing the item name, and add a test for a padded, mixed-case query.",
  ]);
  await expect(page.getByRole("button", { name: "Finish review (1)" })).toBeVisible({ timeout: 10_000 });
  await settle(page, 800);
  await capture(page, {
    name: "25-comment-card-width-01-wide",
    screen: true,
    title: "Long agent comment on a 2000px-wide window",
    expectations: [
      "The agent comment card is capped well short of the diff's full width",
      "The comment text wraps inside the card onto several lines",
    ],
  });
});
