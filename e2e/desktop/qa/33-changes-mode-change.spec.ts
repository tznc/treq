// A file whose diff holds no hunks is its header alone, with no collapse
// chevron and no empty state, and the header says what changed: here a
// chmod and a new empty file, above a file with a diff.
import { chmodSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { expect, test } from "@playwright/test";
import { capture } from "./capture.ts";
import { fixture } from "../fixture.ts";
import { workspaceDir } from "../landing/seed.ts";
import { openTab, openWorkspace, settle } from "./helpers.ts";

const BRANCH = "feat/alerting-logs";

test("changes: a mode change shows its header alone", async ({ page }) => {
  const dir = workspaceDir(fixture().repo, BRANCH);
  chmodSync(join(dir, "README.md"), 0o755);
  writeFileSync(join(dir, "notes.md"), "# Notes\n\nA file with a diff.\n");
  writeFileSync(join(dir, "empty.txt"), "");
  await openWorkspace(page, BRANCH);
  await openTab(page, "Changes");
  const readme = page.getByRole("group", { name: "README.md" });
  await expect(readme.getByLabel("File mode 100644 → 100755")).toBeVisible();
  const empty = page.getByRole("group", { name: "empty.txt" });
  await expect(empty.getByLabel("Empty file")).toBeVisible();
  await expect(page.getByRole("group", { name: "notes.md" })).toBeVisible();
  await expect(page.getByText("No diff hunks")).toHaveCount(0);
  await settle(page, 1500);
  await capture(page, {
    name: "33-changes-mode-change-01-header-alone",
    title: "Changes tab: a chmod and an empty file next to a file with a diff",
    expectations: [
      "README.md and empty.txt are header-only cards with rounded bottoms and no chevron; notes.md keeps its chevron",
      "README.md's header shows a 'File mode 100644 → 100755' badge and empty.txt's an 'Empty file' badge",
      "No 'No diff hunks' empty state anywhere",
    ],
  });
});
