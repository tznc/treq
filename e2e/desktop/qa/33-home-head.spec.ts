// The home checkout's ref, which the sidebar's home row and the header both
// show: a fetch that moves main keeps a clean home on main (its files
// follow), and a detached HEAD reads the same commit in both places.
import { mkdtempSync, existsSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { expect, type Page, test } from "@playwright/test";
import { fixture, git } from "../fixture.ts";
import { gitIn } from "../harness.ts";
import { ALERTING } from "../landing/seed.ts";
import { capture } from "./capture.ts";
import { openWorkspace, ready, row, settle } from "./helpers.ts";

test.describe.configure({ mode: "serial" });

const headBranch = () => git("rev-parse", "--abbrev-ref", "HEAD").trim();
const headId = () => git("rev-parse", "HEAD").trim();
const headerBranch = (page: Page) => page.getByTestId("workspace-branch").textContent();

/** Lands a commit on origin's main from another clone; returns its id. */
function advanceOriginMain(): string {
  const { repo, home } = fixture();
  const clone = mkdtempSync(join(tmpdir(), "treq-qa-upstream-"));
  gitIn(clone, home, "clone", "-q", join(dirname(repo), "origin.git"), ".");
  writeFileSync(join(clone, "CHANGELOG.md"), "# Changelog\n\n- Upstream fix landed on main.\n");
  gitIn(clone, home, "add", ".");
  gitIn(clone, home, "commit", "-q", "-m", "docs: start a changelog");
  gitIn(clone, home, "push", "-q", "origin", "main");
  return gitIn(clone, home, "rev-parse", "HEAD").trim();
}

/** Waits up to `ms` for `done`, without failing: the shot shows what happened. */
async function waitFor(page: Page, done: () => boolean | Promise<boolean>, ms: number) {
  const deadline = Date.now() + ms;
  while (Date.now() < deadline && !(await done())) await page.waitForTimeout(500);
}

test("home-head: a fetch that moves main keeps home on main", async ({ page }) => {
  await ready(page);
  expect(headBranch()).toBe("main");
  expect(headId()).toBe(git("rev-parse", "origin/main").trim());

  const tip = advanceOriginMain();
  // Opening home loads its header, whose status read fetches origin.
  await openWorkspace(page, ALERTING);
  await row(page, "main").first().click();
  await waitFor(page, () => headBranch() === "main" && headId() === tip, 30_000);
  await settle(page, 3000);
  await capture(page, {
    name: "33-01-home-follows-fetched-main",
    title: "Home after a fetch moved origin/main",
    expectations: [
      "The sidebar's event-bus home row reads main, not a commit hash",
      "The header beside the branch icon reads main",
      "The Code tab's file tree lists CHANGELOG.md, the upstream commit's file",
    ],
  });

  expect(headBranch()).toBe("main");
  expect(headId()).toBe(tip);
  expect(existsSync(join(fixture().repo, "CHANGELOG.md"))).toBe(true);
  await expect(row(page, "main").first()).toBeVisible();
  expect(await headerBranch(page)).toBe("main");
});

test("home-head: a detached home shows one commit in the sidebar and header", async ({ page }) => {
  await ready(page);
  git("checkout", "-q", "--detach", "main~1");
  const short = headId().slice(0, 12);
  await waitFor(page, async () => (await row(page, short).count()) > 0, 30_000);
  await row(page, short).first().click();
  await waitFor(page, async () => (await headerBranch(page)) === short, 15_000);
  await settle(page, 2000);
  await capture(page, {
    name: "33-02-detached-home-same-ref",
    title: "Home with Git HEAD detached one commit behind main",
    expectations: [
      `The sidebar's event-bus home row reads the commit ${short}`,
      `The header beside the branch icon reads the same ${short}, not main`,
    ],
  });

  await expect(row(page, short).first()).toBeVisible();
  expect(await headerBranch(page)).toBe(short);
  git("switch", "-q", "main");
});
