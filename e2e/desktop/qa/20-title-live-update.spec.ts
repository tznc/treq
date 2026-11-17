// A workspace title set from the CLI shows in an open window at once.
import { execFileSync } from "node:child_process";
import { readdirSync, readFileSync, statSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test } from "@playwright/test";
import { appBinary } from "../env.ts";
import { capture } from "./capture.ts";
import { openWorkspace, settle } from "./helpers.ts";

const BRANCH = "feat/empty-event-message";
const TITLE = "Renamed from the CLI while the window is open";

/** The running QA app's fixture: its root and repository. */
function qaFixture(): { root: string; repo: string } {
  const root = readdirSync(tmpdir())
    .filter((d) => d.startsWith("treq-qa-"))
    .map((d) => join(tmpdir(), d))
    .filter((d) => {
      try {
        return statSync(join(d, "qa-extra.json")).isFile();
      } catch {
        return false;
      }
    })
    .sort((a, b) => statSync(b).mtimeMs - statSync(a).mtimeMs)[0];
  const { repo } = JSON.parse(readFileSync(join(root, "qa-extra.json"), "utf8"));
  return { root, repo };
}

test("title: a CLI update shows in the open window", async ({ page }) => {
  const { repo } = qaFixture();
  const treq = (...args: string[]) =>
    execFileSync(appBinary, args, { cwd: repo, encoding: "utf8", stdio: "pipe" });
  await openWorkspace(page, BRANCH);
  await settle(page, 1500);

  await capture(page, {
    name: "20-title-01-before",
    title: "Before: the workspace's title",
    expectations: ["The header shows the workspace's original title"],
  });

  const list = JSON.parse(treq("workspace", "list", "--repo", repo));
  const items = Array.isArray(list) ? list : (list.workspaces ?? list.data ?? []);
  const ws = items.find((w: { branch_name?: string; branchName?: string }) =>
    (w.branch_name ?? w.branchName) === BRANCH,
  );
  treq("workspace", "update", "--repo", repo, "--workspace", String(ws.id), "--title", TITLE);

  await expect(page.getByText(TITLE).first()).toBeVisible({ timeout: 5000 });
  await settle(page, 800);
  await capture(page, {
    name: "20-title-02-after",
    title: "After `treq workspace update --title`, no interaction",
    expectations: ["The header shows the new title set from the CLI, untruncated"],
  });
});
