// The Commits tab's count follows history rewritten outside treq: a commit
// made from another process (as an agent in the terminal would) shows in
// both the badge and the list without leaving the tab.
import { execFileSync } from "node:child_process";
import { join } from "node:path";
import { test } from "@playwright/test";
import { fixture } from "../fixture.ts";
import { appBinary } from "../env.ts";
import { appEnv } from "../harness.ts";
import { CACHE, writeInWorkspace } from "../landing/seed.ts";
import { capture } from "./capture.ts";
import { openTab, openWorkspace, settle } from "./helpers.ts";

test("commits count: follows a commit made outside treq", async ({ page }) => {
  await openWorkspace(page, CACHE);
  await openTab(page, "Commits");
  await settle(page, 1500);
  await capture(page, {
    name: "21-commits-count-before",
    screen: true,
    title: "Commits tab before an outside commit",
    expectations: ["The Commits tab's badge equals the number of the workspace's own commit rows"],
  });

  const f = fixture();
  writeInWorkspace(f.repo, CACHE, "outside.txt", "written by an agent\n");
  execFileSync(appBinary, ["commit", CACHE, "-m", "chore: commit made outside treq"], {
    cwd: f.repo,
    env: {
      ...appEnv({ ...f, bin: join(f.root, "bin") }),
      TREQ_APP_DATA_DIR: join(f.home, ".local/share/com.treq"),
    },
    stdio: "pipe",
  });

  await settle(page, 3000);
  await capture(page, {
    name: "21-commits-count-after",
    screen: true,
    title: "Commits tab after an outside commit, without switching tabs",
    expectations: [
      "'chore: commit made outside treq' sits on top of the stack",
      "The Commits tab's badge is one more than before and equals the number of own commit rows",
    ],
  });
});
