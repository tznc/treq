// Committed file rows: the status letter keeps the same inset from the
// row's right edge as the file name has from its left edge.
import { execFileSync } from "node:child_process";
import { join } from "node:path";
import { expect, test } from "@playwright/test";
import { appBinary } from "../env.ts";
import { fixture, git } from "../fixture.ts";
import { appEnv } from "../harness.ts";
import { writeInWorkspace } from "../landing/seed.ts";
import { capture } from "./capture.ts";
import { openTab, openWorkspace, settle } from "./helpers.ts";

const BRANCH = "feat/row-padding";

test("committed rows: status letter is inset from the right edge", async ({ page }) => {
  const f = fixture();
  const treq = (...args: string[]) =>
    execFileSync(appBinary, args, {
      cwd: f.repo,
      env: {
        ...appEnv({ ...f, bin: join(f.root, "bin") }),
        TREQ_APP_DATA_DIR: join(f.home, ".local/share/com.treq"),
      },
      stdio: "pipe",
    });
  git("commit", "--allow-empty", "-m", "chore: base");
  treq("add", BRANCH);
  writeInWorkspace(f.repo, BRANCH, "README.md", "# demo\n\nchanged\n");
  writeInWorkspace(f.repo, BRANCH, "docs/keda.md", "# keda\n");
  writeInWorkspace(f.repo, BRANCH, "docs/logflare.md", "# logflare\n");
  treq("commit", BRANCH, "-m", "docs: update");

  await openWorkspace(page, BRANCH);
  await openTab(page, "Changes");
  await settle(page, 1500);
  const row = page.getByRole("button", { name: "docs/keda.md" }).first();
  await expect(row).toBeVisible();
  await row.click();
  await settle(page, 500);
  const box = (await row.boundingBox())!;
  const clip = {
    x: Math.max(0, box.x - 16),
    y: Math.max(0, box.y - 60),
    width: box.width + 32,
    height: box.height + 120,
  };
  await capture(page, {
    name: "29-committed-row-01-crop",
    title: "Committed list, docs/keda.md selected (cropped)",
    clip,
    expectations: [
      "The selected row's 'A' status letter sits inset from the highlight's right edge",
      "That right inset roughly equals the gap between the highlight's left edge and the file name",
    ],
  });
  await capture(page, {
    name: "29-committed-row-02-full",
    title: "Changes screen with a committed row selected",
    expectations: ["The committed list shows three files, keda.md highlighted"],
  });
});
