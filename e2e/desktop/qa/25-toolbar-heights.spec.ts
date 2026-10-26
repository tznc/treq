// The workspace header's right group: checks, PR pill, sync and Merge share
// one control height, and the sync pip reads clearly in both themes.
import { execFileSync } from "node:child_process";
import { readdirSync, readFileSync, statSync } from "node:fs";
import { tmpdir } from "node:os";
import { DatabaseSync } from "node:sqlite";
import { join } from "node:path";
import { expect, type Page, test } from "@playwright/test";
import { capture } from "./capture.ts";
import { openWorkspace, ready, settle } from "./helpers.ts";

test.describe.configure({ mode: "serial" });

async function headerClip(page: Page) {
  const box = await page.getByRole("button", { name: "More workspace actions" }).boundingBox();
  if (!box) throw new Error("no more button");
  return { x: box.x + box.width - 520, y: box.y - 12, width: 532, height: box.height + 24 };
}

test("toolbar: header controls share one height", async ({ page }) => {
  await ready(page);
  await openWorkspace(page, "feat/empty-event-message");
  await settle(page, 1500);
  await capture(page, {
    name: "25-toolbar-heights",
    title: "Workspace header controls",
    clip: await headerClip(page),
    expectations: [
      "Checks, the PR pill, sync, Merge and the menu share one height and vertical centre",
      "The sync pip is a solid amber dot, not clipped and clear of the button border",
    ],
  });
});

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

// The seed has no remote branches: give the cache workspace a remote ref at
// main and mark it pushed, so its local commits read as ahead and Sync shows.
test("toolbar: header with the sync button", async ({ page }) => {
  const branch = "feat/keyvalues-cache";
  const repo = repoDir();
  execFileSync("git", ["-C", repo, "update-ref", `refs/remotes/origin/${branch}`, "main"]);
  try {
    execFileSync("jj", ["-R", repo, "git", "import"]);
  } catch {
    // Without jj on PATH, the app imports the ref when it next reads.
  }
  const db = new DatabaseSync(join(repo, ".treq", "local.db"));
  db.prepare("UPDATE workspaces SET not_on_remote = 0 WHERE branch_name = ?").run(branch);
  db.close();
  await openWorkspace(page, branch);
  await settle(page, 2000);
  await expect(page.getByRole("button", { name: /^Sync with remote/ })).toBeVisible();
  await capture(page, {
    name: "25-toolbar-sync",
    title: "Workspace header with Sync",
    clip: await headerClip(page),
    expectations: [
      "The PR pill, the ahead count with the sync icon, Merge and the menu share one height and vertical centre",
      "A solid amber pip sits on the sync button's top-right corner, ringed in the background, not clipped",
    ],
  });
});
