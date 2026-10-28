// A rebase never runs over uncommitted work: committing to a stack parent
// rebases its children, and a child with uncommitted changes is held back
// with a warning on its Code tab that points at the Changes tab.
import { execFileSync } from "node:child_process";
import { join } from "node:path";
import { expect, test } from "@playwright/test";
import { fixture } from "../fixture.ts";
import { appBinary } from "../env.ts";
import { appEnv } from "../harness.ts";
import { MARKETING, STACK_PARENT, writeInWorkspace } from "../landing/seed.ts";
import { capture } from "./capture.ts";
import { openTab, openWorkspace, settle } from "./helpers.ts";

test("rebase hold: a dirty child is not rebased and its Code tab says why", async ({ page }) => {
  const f = fixture();
  writeInWorkspace(f.repo, MARKETING, "packages/web/src/wip.ts", "export const WIP = 1;\n");
  writeInWorkspace(f.repo, STACK_PARENT, "packages/api/src/hold.ts", "export const HOLD = 1;\n");
  execFileSync(appBinary, ["commit", STACK_PARENT, "-m", "feat: parent moves on"], {
    cwd: f.repo,
    env: {
      ...appEnv({ ...f, bin: join(f.root, "bin") }),
      TREQ_APP_DATA_DIR: join(f.home, ".local/share/com.treq"),
    },
    stdio: "pipe",
  });

  await openWorkspace(page, MARKETING);
  await openTab(page, "Code");
  await settle(page, 1500);
  await expect(page.getByText("Rebase skipped: uncommitted changes")).toBeVisible();
  await capture(page, {
    name: "27-01-rebase-held-dirty",
    title: "Code tab of a stacked workspace whose rebase was held for uncommitted changes",
    expectations: [
      "An amber alert at the top reads 'Rebase skipped: uncommitted changes'",
      "The alert explains to commit or discard changes and has an 'Open Changes' button",
      "The rest of the Code tab (task box, files, history) renders below it normally",
    ],
  });

  await page.getByRole("button", { name: "Open Changes" }).click();
  await settle(page, 800);
  await expect(page.getByText("wip.ts").first()).toBeVisible();
});
