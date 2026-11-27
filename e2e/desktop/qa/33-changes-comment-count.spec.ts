// The Changes tab's action bar counts open GitHub and local review
// comments as "N comments"; hovering the count lists how many came from
// each source.
import { execFileSync } from "node:child_process";
import { join } from "node:path";
import { expect, test } from "@playwright/test";
import { appEnv } from "../harness.ts";
import { appBinary } from "../env.ts";
import { fixture } from "../fixture.ts";
import { capture } from "./capture.ts";
import { openTab, openWorkspace, popupBox, settle, xPointer } from "./helpers.ts";

const BRANCH = "feat/empty-event-message";

/** `treq` against the QA app's data, as the AI review agent runs it. */
function treq(...args: string[]): string {
  const f = fixture();
  return execFileSync(appBinary, args, {
    cwd: f.repo,
    env: {
      ...appEnv({ ...f, bin: join(f.root, "bin") }),
      TREQ_APP_DATA_DIR: join(f.home, ".local/share/com.treq"),
    },
    encoding: "utf8",
    stdio: "pipe",
  });
}

function workspaceId(): number {
  const list = JSON.parse(treq("workspace", "list", "--repo", fixture().repo));
  const items = Array.isArray(list) ? list : (list.workspaces ?? list.data ?? []);
  return items.find(
    (w: { branch_name?: string; branchName?: string }) => (w.branch_name ?? w.branchName) === BRANCH,
  ).id;
}

test("changes: comment count and its source tooltip", async ({ page }) => {
  await openWorkspace(page, BRANCH);
  await openTab(page, "Changes");
  const count = page.getByTestId("review-comment-count");
  // PR #418's one open GitHub thread; its resolved thread is left out.
  await expect(count).toHaveAccessibleName("1 comment", { timeout: 10_000 });

  treq(
    "agent-review", "add", "--target-type", "workspace_diff", "--target-id", String(workspaceId()),
    "--file", "packages/web/src/lib/client.ts", "--start-line", "6", "--end-line", "6",
    "--comment", "Fall back to an empty string rather than stringifying the whole body.",
  );
  // Agent comments are read every 10s.
  await expect(count).toHaveAccessibleName("2 comments", { timeout: 15_000 });
  await settle(page, 800);
  await capture(page, {
    name: "33-comment-count-01-action-bar",
    title: "Action bar: one GitHub and one local comment",
    expectations: [
      "The action bar reads '2 comments' left of the Review button",
      "The count and button sit on one row, vertically centred",
    ],
  });

  const box = await count.boundingBox();
  expect(box).not.toBeNull();
  await xPointer(page, box!.x + box!.width / 2, box!.y + box!.height / 2);
  await expect.poll(() => popupBox(), { timeout: 3000 }).not.toBeNull();
  await capture(page, {
    name: "33-comment-count-02-tooltip",
    title: "Hovering the count shows its sources",
    screen: true,
    rest: false,
    expectations: [
      "A tooltip next to '2 comments' reads '1 GitHub comment, 1 local review comment'",
      "The tooltip text sits on one line, not clipped",
    ],
  });
});
