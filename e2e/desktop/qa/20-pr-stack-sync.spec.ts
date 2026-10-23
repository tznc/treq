// Stacks follow GitHub: when the pull request under a stacked workspace is
// merged, the PR status poll moves the workspace onto the merged
// workspace's own target, and the sidebar stops nesting it. Runs last: the
// move stays in the demo repository.
import { rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { expect, test } from "@playwright/test";
import { fixture } from "../fixture.ts";
import { capture } from "./capture.ts";
import { home, ready, settle } from "./helpers.ts";

const PARENT = "feat/event-ingest";
const CHILD = "feat/empty-event-message";
const overrides = () => join(fixture().home, "fake-gh-prs.json");

test.describe.configure({ mode: "serial" });
test.afterAll(() => rmSync(overrides(), { force: true }));

/** The child workspace's rebase target, from the repository's database. */
function childTarget(): string | null {
  const db = new DatabaseSync(join(fixture().repo, ".treq/local.db"));
  const row = db
    .prepare("SELECT target_branch FROM workspaces WHERE branch_name = ?")
    .get(CHILD) as { target_branch: string | null } | undefined;
  db.close();
  return row?.target_branch ?? null;
}

test("pr-stack-sync: a merged base PR unstacks its child", async ({ page }) => {
  test.setTimeout(240_000);
  await ready(page);
  await home(page);
  await settle(page, 3000);
  expect(childTarget()).toBe(PARENT);
  await capture(page, {
    name: "20-pr-stack-sync-before",
    title: "Stacked workspace before its base PR merges",
    clip: { x: 0, y: 0, width: 300, height: 520 },
    expectations: [
      `${CHILD} is indented under ${PARENT} in the WORKSPACES list`,
    ],
  });

  // #412 (feat/event-ingest) merges on GitHub; the next PR poll sees it.
  writeFileSync(overrides(), JSON.stringify({ [PARENT]: { state: "MERGED" } }));
  await expect
    .poll(childTarget, { timeout: 180_000, intervals: [2000] })
    .toBe("main");
  await settle(page, 1500);
  await capture(page, {
    name: "20-pr-stack-sync-after",
    title: "The child moved off the merged base",
    clip: { x: 0, y: 0, width: 300, height: 520 },
    expectations: [
      `${CHILD} sits at the top level, aligned with ${PARENT}, no longer indented`,
      `${PARENT} shows the merged (purple) branch icon; no row is clipped or overlapping`,
    ],
  });
});
