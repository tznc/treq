// A committed file's card offers "Add file comment", and the comment
// lands on the file like it does for uncommitted files.
import { execFileSync } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { expect, test } from "@playwright/test";
import { appBinary } from "../env.ts";
import { fixture, git } from "../fixture.ts";
import { appEnv } from "../harness.ts";
import { writeInWorkspace } from "../landing/seed.ts";
import { capture } from "./capture.ts";
import { openTab, openWorkspace, settle } from "./helpers.ts";

const BRANCH = "feat/committed-comment";
const FILE = "packages/api/src/backends.ts";

test("committed file comment: button shows and a comment saves", async ({ page }) => {
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
  mkdirSync(dirname(join(f.repo, FILE)), { recursive: true });
  writeFileSync(join(f.repo, FILE), 'export const backend = "a";\n');
  git("add", FILE);
  git("commit", "-m", "feat: backends");
  treq("add", BRANCH);
  writeInWorkspace(f.repo, BRANCH, FILE, 'export const backend = "b";\nexport const extra = 1;\n');
  treq("commit", BRANCH, "-m", "feat: change backend");

  await openWorkspace(page, BRANCH);
  await openTab(page, "Changes");
  await settle(page, 1500);
  const button = page.getByRole("button", { name: `Add file comment on ${FILE}` });
  await expect(button).toBeVisible();
  await capture(page, {
    name: "29-committed-comment-01-button",
    title: "Committed file card shows 'Add file comment'",
    expectations: [
      "backends.ts card carries the 'Committed' badge",
      "A message-plus icon button sits left of the 'Viewed' checkbox in the card header",
    ],
  });

  await button.click();
  await settle(page, 500);
  await page.keyboard.type("Split this file");
  await settle(page, 300);
  await capture(page, {
    name: "29-committed-comment-02-draft",
    title: "File comment draft open on a committed file",
    expectations: ["A comment input under the backends.ts header holds 'Split this file'"],
  });
  await page.keyboard.press("Control+Enter");
  await settle(page, 800);
  await capture(page, {
    name: "29-committed-comment-03-saved",
    title: "File comment saved on a committed file",
    expectations: [
      "A file comment card reading 'Split this file' sits under the backends.ts header",
      "The review action bar is visible",
    ],
  });
});
