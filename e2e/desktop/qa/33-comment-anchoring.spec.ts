// Review comments follow their lines as GitHub threads do: a comment moves
// when lines are added above it, shows as "Not in diff" when its lines are
// unchanged but leave the diff, and goes outdated once its own code
// changes. Covers the review's pending comments and, once sent to an agent,
// the sent ones. Runs on the alerting workspace's uncommitted edits
// (qa/launch.ts).
import { readdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test, type Page } from "@playwright/test";
import { workspaceDir } from "../landing/seed.ts";
import { capture } from "./capture.ts";
import { openTab, openWorkspace, settle } from "./helpers.ts";

test.describe.configure({ mode: "serial" });

const ALERTING = "feat/alerting-logs";
const FILE = "packages/api/src/alerting.ts";
const README = "README.md";

/** alerting.ts as qa/launch.ts leaves it uncommitted, under `prefix`. */
const alerting = (prefix: string, level = "error") =>
  `${prefix}export function formatAlert(line: string, level = "info"): string {
  return \`[lorem:\${level}] \${line}\`;
}

export function formatError(line: string): string {
  return formatAlert(line, "${level}");
}
`;

/** README.md as seeded, with lines rewritten by number. */
function readme(original: string, edits: Record<number, string>): string {
  const lines = original.split("\n");
  for (const [n, text] of Object.entries(edits)) lines[Number(n) - 1] = text;
  return lines.join("\n");
}

/** The checkout of the running QA app's alerting workspace. */
function alertingDir(): string {
  const roots = readdirSync(tmpdir())
    .filter((d) => d.startsWith("treq-qa-"))
    .map((d) => join(tmpdir(), d))
    .filter((d) => statSync(join(d, "qa-extra.json"), { throwIfNoEntry: false })?.isFile() ?? false)
    .sort((a, b) => statSync(b).mtimeMs - statSync(a).mtimeMs);
  const root = roots[0];
  if (!root) throw new Error("no QA fixture");
  const { repo } = JSON.parse(readFileSync(join(root, "qa-extra.json"), "utf8"));
  return workspaceDir(repo, ALERTING);
}

const write = (path: string, text: string) => writeFileSync(join(alertingDir(), path), text);
const read = (path: string) => readFileSync(join(alertingDir(), path), "utf8");

/** Comments `text` on line `line` of `path` through its gutter's add button. */
async function commentOn(page: Page, path: string, line: number, text: string): Promise<void> {
  const add = page.getByRole("button", { name: `Add comment on ${path} line ${line}` }).last();
  await add.hover();
  await add.click();
  const input = page.getByRole("textbox", { name: "Add a comment…" });
  await expect(input).toBeVisible();
  await input.fill(text);
  await page.getByRole("button", { name: "Add comment", exact: true }).click();
  await expect(input).toHaveCount(0);
}

/** Reloads the diff behind the stale banner an edit on disk raises. */
async function reload(page: Page): Promise<void> {
  const button = page.getByRole("button", { name: "Reload" });
  await expect(button).toBeVisible({ timeout: 15_000 });
  await button.click();
  await settle(page, 2000);
}

test("comment anchoring: pending comments move, go off the diff", async ({ page }) => {
  // README.md's last line changed, so its line 7 is context in the diff.
  const original = read(README);
  write(README, readme(original, { 10: "Edited last line." }));
  await openWorkspace(page, ALERTING);
  await openTab(page, "Changes");
  await settle(page, 1500);

  await commentOn(page, FILE, 6, "Pass the level through");
  await expect(page.getByRole("group", { name: `Comments on ${FILE} line 6` })).toBeVisible();

  // Two lines added above it.
  write(FILE, alerting("// Alert formatting.\n\n"));
  await reload(page);
  await expect(page.getByRole("group", { name: `Comments on ${FILE} line 8` })).toBeVisible({
    timeout: 15_000,
  });
  await page.getByRole("group", { name: `Comments on ${FILE} line 8` }).scrollIntoViewIfNeeded();
  await settle(page, 800);
  await capture(page, {
    name: "33-anchor-01-pending-moved",
    title: "A pending comment follows its line when lines are added above it",
    expectations: [
      "The comment 'Pass the level through' sits under `return formatAlert(line, \"error\");`",
      "That line is numbered 8 now (two lines were added at the top of alerting.ts)",
      "No Outdated section in alerting.ts",
    ],
  });

  // A context line of README.md: the change near it is undone and another
  // made at the top, so its line is unchanged but no longer in the diff.
  await commentOn(page, README, 7, "Keep this paragraph");
  write(README, readme(original, { 1: "# Edited title" }));
  await reload(page);
  // Its badge text is not observable: the capture shows which.
  const offDiff = page
    .getByRole("group", { name: README, exact: true })
    .getByRole("group", { name: "Outdated comments" });
  await expect(offDiff).toBeVisible({ timeout: 15_000 });
  await offDiff.scrollIntoViewIfNeeded();
  await settle(page, 800);
  await capture(page, {
    name: "33-anchor-02-not-in-diff",
    title: "A comment whose lines left the diff, unchanged, shows as Not in diff",
    expectations: [
      "README.md's card starts with a comments section holding 'Keep this paragraph'",
      "That card is badged 'Not in diff' (not 'Outdated') and labelled Line 7",
      "The diff below shows only the change to the first line (lines 1-4)",
    ],
  });
});

test("comment anchoring: sent comments move, then go outdated", async ({ page }) => {
  await openWorkspace(page, ALERTING);
  await openTab(page, "Changes");
  await page.getByRole("button", { name: "Finish review" }).click();
  await page.getByRole("button", { name: "Plan", exact: true }).click();
  await settle(page, 2500);
  // The agent's terminal opens below the diff: fold it so the diff has room.
  await page
    .getByRole("toolbar", { name: "Terminal pane controls" })
    .getByRole("button", { name: "Collapse" })
    .click();
  await openTab(page, "Changes");
  await settle(page, 1500);
  const sent = (line: number) =>
    page.getByRole("group", { name: `Sent comment on ${FILE} line ${line}` });
  await expect(sent(8)).toBeVisible({ timeout: 15_000 });

  // One more line above it: no review is open, so the diff follows at once.
  write(FILE, alerting("// Alert formatting.\n// Levels: info, warn, error.\n\n"));
  await expect(sent(9)).toBeVisible({ timeout: 20_000 });
  await sent(9).scrollIntoViewIfNeeded();
  await settle(page, 800);
  await capture(page, {
    name: "33-anchor-03-sent-moved",
    title: "A sent comment follows its line like a GitHub thread",
    expectations: [
      "A 'Sent to agent' card reading 'Pass the level through' sits under line 9",
      "Line 9 is `return formatAlert(line, \"error\");`",
    ],
  });

  // Its own line changes: it leaves the line for the outdated section.
  write(FILE, alerting("// Alert formatting.\n// Levels: info, warn, error.\n\n", "warn"));
  const outdated = page.getByRole("group", { name: `Outdated threads on ${FILE}` });
  await expect(
    outdated.getByRole("group", { name: `Sent comment on ${FILE} line 9` }),
  ).toBeVisible({ timeout: 20_000 });
  await outdated.scrollIntoViewIfNeeded();
  await settle(page, 800);
  await capture(page, {
    name: "33-anchor-04-sent-outdated",
    title: "A sent comment whose line changed is outdated, not left on other code",
    expectations: [
      "alerting.ts's card starts with an outdated section holding the 'Sent to agent' card",
      "That card is badged 'Outdated'",
    ],
  });
  // Down the diff, past the outdated section to the rewritten line.
  await page.mouse.move(1000, 700);
  await page.mouse.wheel(0, 400);
  await settle(page, 800);
  await capture(page, {
    name: "33-anchor-05-line-left-bare",
    title: "The rewritten line no longer carries the outdated comment",
    expectations: [
      "Line 9 of alerting.ts reads `return formatAlert(line, \"warn\");`",
      "No comment card sits under line 9 or any other line of alerting.ts",
    ],
  });
});
