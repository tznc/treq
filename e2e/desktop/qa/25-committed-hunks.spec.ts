// A file both committed and dirty shows one diff from the fork point. Its
// hunks with no uncommitted change carry a "Committed" label, and with
// Committed hidden they shut to their header until the label opens them.
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

const BRANCH = "feat/committed-hunks";
const FILE = "packages/api/src/routes.ts";

const lines = (edit: (n: number) => string) =>
  Array.from({ length: 60 }, (_, i) => edit(i + 1)).join("\n") + "\n";
const route = (n: number, v = 1) => `export const route${n} = "/v${v}/route-${n}";`;
const MAIN = lines((n) => route(n));
// Committed: line 10 moves to v2 and gains a comment.
const COMMITTED = lines((n) => (n === 10 ? `${route(10, 2)}\n// v2 from here` : route(n)));
// Uncommitted on top: line 40 moves to v2 too.
const DIRTY = lines((n) =>
  n === 10 ? `${route(10, 2)}\n// v2 from here` : n === 40 ? route(40, 2) : route(n),
);

test("committed hunks: labelled, and shut while Committed is hidden", async ({ page }) => {
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
  writeFileSync(join(f.repo, FILE), MAIN);
  git("add", FILE);
  git("commit", "-m", "feat: list routes");
  treq("add", BRANCH);
  writeInWorkspace(f.repo, BRANCH, FILE, COMMITTED);
  treq("commit", BRANCH, "-m", "feat: route 10 on v2");
  writeInWorkspace(f.repo, BRANCH, FILE, DIRTY);

  await openWorkspace(page, BRANCH);
  await openTab(page, "Changes");
  await settle(page, 1500);
  await expect(page.getByTestId(`committed-hunk-${FILE}-0`)).toBeVisible();
  await expect(page.getByTestId(`committed-hunk-${FILE}-1`)).toHaveCount(0);
  await capture(page, {
    name: "25-committed-hunks-01-labelled",
    title: "routes.ts, committed and dirty, with Committed shown",
    expectations: [
      "The first hunk's header (line 10) carries a 'Committed' badge after its header text",
      "The second hunk (line 40, uncommitted) has no badge",
      "Both hunks show their lines",
    ],
  });

  await page.getByTestId("committed-hide").click();
  await settle(page, 1500);
  const open = page.getByRole("button", { name: "Show committed hunk" });
  await expect(open).toBeVisible();
  await capture(page, {
    name: "25-committed-hunks-02-shut",
    title: "The same file with Committed hidden",
    expectations: [
      "The first hunk shows only its header, with a right chevron and the 'Committed' badge",
      "The second hunk (line 40) shows its lines as before",
    ],
  });

  await open.click();
  await settle(page, 1000);
  await expect(page.getByRole("button", { name: "Hide committed hunk" })).toBeVisible();
  await capture(page, {
    name: "25-committed-hunks-03-opened",
    title: "The committed hunk opened from its label",
    expectations: [
      "The first hunk shows its lines again, its badge now with a down chevron",
    ],
  });
});
