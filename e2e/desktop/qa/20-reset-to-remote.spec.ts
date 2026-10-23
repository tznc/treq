// The workspace header's More menu offers Reset to remote above Delete
// Workspace once the branch is on origin, and asks before discarding the
// local commits. The seed has no remote branches, so this gives the cache
// workspace a remote-tracking ref at main, leaving its commits local-only,
// and marks it pushed.
import { execFileSync } from "node:child_process";
import { readdirSync, readFileSync, statSync } from "node:fs";
import { tmpdir } from "node:os";
import { DatabaseSync } from "node:sqlite";
import { join } from "node:path";
import { expect, test } from "@playwright/test";
import { capture } from "./capture.ts";
import { focusMain, openWorkspace, popupBox, settle, xPointer } from "./helpers.ts";

test.describe.configure({ mode: "serial" });

const BRANCH = "feat/keyvalues-cache";

/** The running QA app's demo repository. */
function repoDir(): string {
  const roots = readdirSync(tmpdir())
    .filter((d) => d.startsWith("treq-qa-"))
    .map((d) => join(tmpdir(), d))
    .filter((d) => {
      try {
        return statSync(join(d, "qa-extra.json")).isFile();
      } catch {
        return false;
      }
    })
    .sort((a, b) => statSync(b).mtimeMs - statSync(a).mtimeMs);
  return JSON.parse(readFileSync(join(roots[0], "qa-extra.json"), "utf8")).repo;
}

test("reset-to-remote: More menu item and its question", async ({ page }) => {
  const repo = repoDir();
  execFileSync("git", ["-C", repo, "update-ref", `refs/remotes/origin/${BRANCH}`, "main"]);
  execFileSync("jj", ["-R", repo, "git", "import"]);
  // What a push records.
  const db = new DatabaseSync(join(repo, ".treq", "local.db"));
  db.prepare("UPDATE workspaces SET not_on_remote = 0 WHERE branch_name = ?").run(BRANCH);
  db.close();
  await openWorkspace(page, BRANCH);
  await settle(page, 2000);
  await page.getByRole("button", { name: "More workspace actions" }).click();
  await settle(page, 900);
  focusMain();
  const menu = popupBox();
  if (!menu) throw new Error("no More menu");
  await capture(page, {
    name: "20-reset-to-remote-menu",
    title: "More menu with Reset to remote",
    screen: true,
    rest: false,
    expectations: [
      "The menu lists 'Reset to remote' with a counter-clockwise arrow, a separator, then 'Delete Workspace'",
      "Both items are red, as destructive items are",
    ],
  });
  // The first row.
  await xPointer(page, menu.x + menu.w / 2, menu.y + 18, 1);
  const question = page.getByRole("dialog", { name: `Reset ${BRANCH} to origin?`, exact: true });
  await expect(question).toBeVisible();
  await expect(page.getByTestId("confirm-confirm")).toHaveAccessibleName("Reset to remote");
  await settle(page, 600);
  await capture(page, {
    name: "20-reset-to-remote-confirm",
    title: "Reset to remote: the question",
    screen: true,
    rest: false,
    expectations: [
      `The title asks 'Reset ${BRANCH} to origin?' and the text names the local commits it discards`,
      "Cancel and a red 'Reset to remote' button; no Yes/No",
    ],
  });
  await page.keyboard.press("Escape");
  await expect(question).toHaveCount(0);
});
