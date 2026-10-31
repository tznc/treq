// A rebase never moves a workspace onto a parent with uncommitted changes:
// the child's Code tab says which parent holds it back. The hold is stored
// with the workspace (`metadata.rebase_hold`), as the rebase records it.
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { expect, test } from "@playwright/test";
import { fixture } from "../fixture.ts";
import { MARKETING, STACK_PARENT } from "../landing/seed.ts";
import { capture } from "./capture.ts";
import { openTab, openWorkspace, settle } from "./helpers.ts";

function holdRebase(repo: string, branch: string, hold: unknown): void {
  const db = new DatabaseSync(join(repo, ".treq", "local.db"));
  const row = db
    .prepare("SELECT metadata FROM workspaces WHERE branch_name = ?")
    .get(branch) as { metadata: string | null };
  const metadata = { ...JSON.parse(row.metadata ?? "{}"), rebase_hold: hold };
  db.prepare("UPDATE workspaces SET metadata = ? WHERE branch_name = ?").run(
    JSON.stringify(metadata),
    branch,
  );
  db.close();
}

test("rebase hold: a child of a dirty parent says why it was not rebased", async ({ page }) => {
  const f = fixture();
  holdRebase(f.repo, MARKETING, { ParentDirty: { branch: STACK_PARENT } });

  await openWorkspace(page, MARKETING);
  await openTab(page, "Code");
  await settle(page, 1500);
  await expect(page.getByText(`Rebase skipped: ${STACK_PARENT} has uncommitted changes`)).toBeVisible();
  await capture(page, {
    name: "27-01-rebase-held-parent-dirty",
    title: "Code tab of a stacked workspace held back by its dirty parent",
    expectations: [
      `An amber alert reads 'Rebase skipped: ${STACK_PARENT} has uncommitted changes'`,
      `The alert tells the user to commit or discard the changes in ${STACK_PARENT}`,
      "The rest of the Code tab (task box, stack, files, history) renders below it normally",
    ],
  });

  holdRebase(f.repo, MARKETING, null);
});
