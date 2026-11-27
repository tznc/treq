// The workspace header's right group: checks, PR pill, sync and Merge share
// one control height, sync always shows, and its pip reads clearly in both
// themes when the branch and its remote differ.
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
  await expect(
    page.getByRole("button", { name: "Sync with remote: Not on remote" }),
  ).toBeVisible();
  await capture(page, {
    name: "25-toolbar-heights",
    title: "Workspace header controls",
    clip: await headerClip(page),
    expectations: [
      "Checks, the PR pill, sync, Merge and the menu share one height and vertical centre",
      "The sync button shows only its icon, with no amber pip: the branch is not on the remote",
    ],
  });
});

/** The running QA app's demo repository. */
function repoDir(): string {
  const roots = readdirSync(tmpdir())
    .filter((d) => d.startsWith("treq-qa-"))
    .map((d) => join(tmpdir(), d))
    .filter((d) => statSync(join(d, "qa-extra.json"), { throwIfNoEntry: false })?.isFile() ?? false)
    .sort((a, b) => statSync(b).mtimeMs - statSync(a).mtimeMs);
  const root = roots[0];
  if (!root) throw new Error("no QA fixture");
  return JSON.parse(readFileSync(join(root, "qa-extra.json"), "utf8")).repo;
}

/**
 * Gives `branch` a remote ref at `remoteRef` and marks it pushed, with a
 * rebase onto its target `hoursAgo` hours ago for the sync tooltip.
 */
function seedRemote(branch: string, remoteRef: string, hoursAgo: number): void {
  const repo = repoDir();
  execFileSync("git", ["-C", repo, "update-ref", `refs/remotes/origin/${branch}`, remoteRef]);
  try {
    execFileSync("jj", ["-R", repo, "git", "import"]);
  } catch {
    // Without jj on PATH, the app imports the ref when it next reads.
  }
  const db = new DatabaseSync(join(repo, ".treq", "local.db"));
  db.prepare("UPDATE workspaces SET not_on_remote = 0 WHERE branch_name = ?").run(branch);
  const rebasedAt = new Date(Date.now() - hoursAgo * 3600_000).toISOString();
  db.prepare(
    "UPDATE workspaces SET metadata = json_set(COALESCE(metadata, '{}'), '$.last_rebased_at', ?) WHERE branch_name = ?",
  ).run(rebasedAt, branch);
  db.close();
}

async function captureTooltip(page: Page, name: string, text: string): Promise<void> {
  await page.getByRole("button", { name: /^Sync with remote/ }).hover();
  await settle(page, 1500);
  await capture(page, {
    name,
    title: "Sync tooltip with the last rebase",
    screen: true,
    rest: false,
    expectations: [
      `Hovering sync shows a tooltip reading '${text}'`,
      "The tooltip is fully on screen and not clipped",
    ],
  });
}

// The seed has no remote branches: give the cache workspace a remote ref at
// main and mark it pushed, so its local commits read as ahead.
test("toolbar: header with the sync button", async ({ page }) => {
  const branch = "feat/keyvalues-cache";
  seedRemote(branch, "main", 3);
  await openWorkspace(page, branch);
  await settle(page, 2000);
  await expect(page.getByRole("button", { name: /^Sync with remote: \d+ to push/ })).toBeVisible();
  await capture(page, {
    name: "25-toolbar-sync",
    title: "Workspace header with Sync",
    clip: await headerClip(page),
    expectations: [
      "The PR pill, the ahead count with the sync icon, Merge and the menu share one height and vertical centre",
      "A solid amber pip sits on the sync button's top-right corner, ringed in the background, not clipped",
    ],
  });
  await captureTooltip(
    page,
    "25-toolbar-sync-tooltip",
    "<n> to push. Last rebased on main 3 hours ago. Sync with remote (fetch and push)",
  );
});

// The alerting workspace's remote ref at its own tip: nothing to push or
// pull, and sync still shows.
test("toolbar: sync on a branch that matches its remote", async ({ page }) => {
  const branch = "feat/alerting-logs";
  seedRemote(branch, `refs/heads/${branch}`, 48);
  await openWorkspace(page, branch);
  await settle(page, 2000);
  await expect(page.getByRole("button", { name: "Sync with remote: Up to date" })).toBeVisible();
  await capture(page, {
    name: "25-toolbar-sync-up-to-date",
    title: "Workspace header with Sync up to date",
    clip: await headerClip(page),
    expectations: [
      "The sync button shows only its icon, with no count and no amber pip",
      "Sync shares one height and vertical centre with its neighbours",
    ],
  });
  await captureTooltip(
    page,
    "25-toolbar-sync-up-to-date-tooltip",
    "Up to date. Last rebased on main 2 days ago. Sync with remote (fetch and push)",
  );
});
