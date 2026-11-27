// Changes tab file rows: the status letter keeps one column across the
// Uncommitted and Committed lists, and a row's discard and stage buttons
// show only on hover or once the row is clicked, until a click elsewhere.
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

const BRANCH = "feat/row-actions";
const FILES = ["lib/backends.ex", "lib/users.ex", "lib/source.ex", "test/test_helper.exs"];

test("changes rows: aligned status letters, actions on hover or click", async ({ page }) => {
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
  for (const file of FILES) {
    const path = join(f.repo, file);
    mkdirSync(dirname(path), { recursive: true });
    writeFileSync(path, `# ${file}\n`);
  }
  git("add", ...FILES);
  git("commit", "-m", "chore: base files");
  treq("add", BRANCH);
  writeInWorkspace(f.repo, BRANCH, "lib/source.ex", "# lib/source.ex\n\ncommitted\n");
  writeInWorkspace(f.repo, BRANCH, "test/test_helper.exs", "# helper\n\ncommitted\n");
  treq("commit", BRANCH, "-m", "feat: committed edits");
  writeInWorkspace(f.repo, BRANCH, "lib/backends.ex", "# lib/backends.ex\n\nwip\n");
  writeInWorkspace(f.repo, BRANCH, "lib/users.ex", "# lib/users.ex\n\nwip\n");

  await openWorkspace(page, BRANCH);
  await openTab(page, "Changes");
  await settle(page, 1500);
  const users = page.getByRole("button", { name: "lib/users.ex", exact: true });
  const selected = page.getByRole("button", { name: "lib/users.ex", exact: true, selected: true });
  await expect(users).toBeVisible();
  const list = (await page.getByRole("button", { name: "lib/backends.ex", exact: true }).boundingBox())!;
  const clip = {
    x: Math.max(0, list.x - 16),
    y: Math.max(0, list.y - 24),
    width: list.width + 32,
    height: 200,
  };

  await capture(page, {
    name: "32-changes-row-actions-01-rest",
    title: "Changes list at rest",
    clip,
    expectations: [
      "Every M letter, Uncommitted and Committed, sits in one column at the rows' right edge",
      "No row shows discard or + buttons",
    ],
  });

  await users.hover();
  await settle(page, 400);
  await capture(page, {
    name: "32-changes-row-actions-02-hover",
    title: "Hovered users.ex row",
    clip,
    rest: false,
    expectations: [
      "users.ex shows discard and + to the left of its M",
      "Its M stays in the same column as the other rows' letters",
    ],
  });

  await users.click();
  await expect(selected).toHaveCount(1);
  await page.mouse.move(900, 600, { steps: 4 }); // off the row: no hover state
  await settle(page, 600);
  await capture(page, {
    name: "32-changes-row-actions-03-clicked",
    title: "Clicked users.ex row, pointer elsewhere",
    clip,
    rest: false,
    expectations: [
      "users.ex stays highlighted with discard and + showing",
      "Its M stays in the same column as the other rows' letters",
    ],
  });

  await page.mouse.click(900, 600); // in the diff: away from the list
  await expect(selected).toHaveCount(0);
  await settle(page, 600);
  await capture(page, {
    name: "32-changes-row-actions-04-clicked-away",
    title: "After a click in the diff",
    clip,
    expectations: [
      "No row is highlighted",
      "No row shows discard or + buttons; the M column is unchanged",
    ],
  });
});
