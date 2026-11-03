// A workspace whose branch is gone (a fetch drops a branch deleted on the
// remote once its PR merges) shows an alert on its Code tab with an Archive
// button, and its diff reads without a "JJ Error". Deleting the bookmark
// stands in for that fetch, and the rebase record the diff compares against
// is what made the read fail.
// Runs late: it archives the alerting workspace for the rest of the run.
import { execFileSync } from "node:child_process";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { expect, test } from "@playwright/test";
import { fixture } from "../fixture.ts";
import { ALERTING } from "../landing/seed.ts";
import { capture } from "./capture.ts";
import { openTab, openWorkspace, row, settle } from "./helpers.ts";

test.describe.configure({ mode: "serial" });

test("branch deleted: Code tab offers Archive, the diff still reads", async ({ page }) => {
  const f = fixture();
  const main = execFileSync("jj", ["-R", f.repo, "log", "--no-graph", "-r", "main", "-T", "commit_id"], {
    encoding: "utf8",
  }).trim();
  const db = new DatabaseSync(join(f.repo, ".treq", "local.db"));
  const { metadata } = db.prepare("SELECT metadata FROM workspaces WHERE branch_name = ?").get(ALERTING) as {
    metadata: string | null;
  };
  const rebased = { ...JSON.parse(metadata ?? "{}"), last_rebased_target_commit: main };
  db.prepare("UPDATE workspaces SET metadata = ? WHERE branch_name = ?").run(JSON.stringify(rebased), ALERTING);
  db.close();
  execFileSync("jj", ["-R", f.repo, "bookmark", "delete", ALERTING]);

  await openWorkspace(page, ALERTING);
  await openTab(page, "Changes");
  await settle(page, 2000);
  await capture(page, {
    name: "98-01-branch-deleted-changes",
    title: "Changes tab of a workspace whose branch was deleted",
    screen: true,
    expectations: [
      "No 'JJ Error' toast shows",
      "The Changes tab lists the workspace's changes instead of an error",
    ],
  });

  await openTab(page, "Code");
  await settle(page, 1500);
  await page.getByRole("button", { name: /^Alerts: .*1 warning/ }).click();
  await expect(page.getByText("Branch deleted on remote")).toBeVisible();
  await capture(page, {
    name: "98-02-branch-deleted-alert",
    title: "Code tab alert for a deleted branch",
    expectations: [
      "The alerts popover is open under the action row, with an amber alert reading 'Branch deleted on remote'",
      `Its message names ${ALERTING} and suggests archiving; an outlined Archive button sits on the alert`,
      "The rest of the Code tab renders normally behind the popover",
    ],
  });

  await page.getByRole("button", { name: "Archive", exact: true }).click();
  await settle(page, 2500);
  await expect(row(page, ALERTING)).toHaveCount(0);
  await capture(page, {
    name: "98-03-branch-deleted-archived",
    title: "After Archive",
    screen: true,
    expectations: [
      `${ALERTING} is gone from the sidebar`,
      "A 'Workspace archived' toast shows, and no error toast",
    ],
  });
});
