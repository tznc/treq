// The Changes tab diff's "Show more lines above / below" rows. An
// uncommitted file whose last hunk reaches the end of the file offers no
// "below" row, and the "above" header goes once the expansion reaches
// line 1. Committed files expand too: see 14-diff-context-committed.
//
// File rows and the context rows are not observable, so those steps click
// fixed points (for this seed and the 1400x900 window); each then settles.
import { appendFileSync } from "node:fs";
import { join } from "node:path";
import { expect, test } from "@playwright/test";
import { capture } from "./capture.ts";
import { fixture } from "../fixture.ts";
import { workspaceDir } from "../landing/seed.ts";
import { openTab, openWorkspace, settle } from "./helpers.ts";

const BRANCH = "feat/alerting-logs";
const FILE = { x: 330, y: 283 }; // README.md under Uncommitted
const ABOVE = { x: 826, y: 204 }; // the hunk's "Show more lines above" header

test("diff context: expand above until the file is fully shown", async ({ page }) => {
  // README.md is 10 lines; two appended lines make one short hunk that
  // ends at the end of the file, with 7 lines hidden above it.
  appendFileSync(
    join(workspaceDir(fixture().repo, BRANCH), "README.md"),
    "\nSee `docs/` for the full guide.\n",
  );
  await openWorkspace(page, BRANCH);
  await openTab(page, "Changes");
  await page.mouse.click(FILE.x, FILE.y);
  await settle(page, 1500);
  await capture(page, {
    name: "14-diff-context-01-hunk-at-end",
    title: "README.md diff: a hunk that ends at the end of the file",
    expectations: [
      "The hunk header offers 'Show more lines above' (earlier lines are hidden)",
      "No 'Show more lines below' row under the last added line",
      "Added lines are green and the last line number is the file's last line",
    ],
  });
  for (let i = 0; i < 4; i += 1) {
    await page.mouse.click(ABOVE.x, ABOVE.y);
    await settle(page, 800);
  }
  await capture(page, {
    name: "14-diff-context-02-fully-shown",
    title: "README.md diff after expanding above to line 1",
    expectations: [
      "No 'Show more lines above' or 'Show more lines below' row remains",
      "Line numbers run continuously from 1 to the last line, no gaps or repeats",
      "Added lines are still green",
    ],
  });
});
