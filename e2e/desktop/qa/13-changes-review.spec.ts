// Reviewing in the Changes tab: character-level marks in a replaced line,
// moving and extending the line selection from the keyboard, a suggested
// change written and applied, and an agent's edit on disk raising the
// stale banner while a review is open. Runs on the alerting workspace's
// uncommitted edits (qa/launch.ts).
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

/** The checkout of the running QA app's alerting workspace. */
function alertingDir(): string {
  const roots = readdirSync(tmpdir())
    .filter((d) => d.startsWith("treq-qa-"))
    .map((d) => join(tmpdir(), d))
    .filter((d) => {
      try {
        return statSync(join(d, "qa-extra.json")).isFile();
      } catch {
        return false;
      }
    })
    .sort((a, b) => statSync(b).mtimeMs - statSync(a).mtimeMs);
  const { repo } = JSON.parse(readFileSync(join(roots[0], "qa-extra.json"), "utf8"));
  return workspaceDir(repo, ALERTING);
}

async function changes(page: Page): Promise<void> {
  await openWorkspace(page, ALERTING);
  await openTab(page, "Changes");
  await settle(page, 1500);
}

test("changes review: character marks, keyboard, suggestion, stale banner", async ({ page }) => {
  await changes(page);
  await capture(page, {
    name: "13-review-01-char-diff",
    title: "Replaced lines mark the characters they changed",
    expectations: [
      "In alerting.ts the removed and added signature lines are paired",
      "Only `, level = \"info\"` and `:${level}` carry the deeper red/green mark",
      "Unchanged parts of those lines keep the plain line background",
    ],
  });

  // The first added line's gutter (not observable, so by place in the
  // 1400x900 window), then the keyboard.
  await page.mouse.click(600, 277);
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("Shift+ArrowDown");
  await settle(page, 500);
  await capture(page, {
    name: "13-review-02-keyboard-selection",
    title: "Down moves the cursor, Shift+Down extends the selection",
    expectations: ["Two consecutive diff lines are highlighted blue as one selection"],
  });

  await page.keyboard.press("ArrowLeft");
  const input = page.getByRole("textbox", { name: "Add a comment..." });
  await expect(input).toBeVisible();
  await page.getByRole("button", { name: "Suggest change" }).click();
  await settle(page, 400);
  await capture(page, {
    name: "13-review-03-suggest-change",
    title: "Left opens the comment box; Suggest change quotes the lines",
    expectations: [
      "Comment box under the selected lines with their range label",
      "Text holds a ```suggestion block with the two lines",
      "Suggest change on the left, Cancel and Add Comment on the right",
    ],
  });
  await input.fill('Wrap it\n```suggestion\n  return `[lorem:${level.toUpperCase()}] ${line}`;\n}\n```');
  await page.getByRole("button", { name: "Add Comment", exact: true }).click();
  await expect(input).toHaveCount(0);
  await settle(page, 800);
  await capture(page, {
    name: "13-review-04-suggestion-card",
    title: "A comment with a suggested change",
    expectations: [
      "Comment text above a 'Suggested change' diff of old vs suggested lines",
      "Changed words marked; Apply suggestion button at the bottom right",
    ],
  });
  await page.getByRole("button", { name: "Edit comment" }).hover();
  await settle(page, 500);
  await capture(page, {
    name: "13-review-04b-comment-card-hover",
    title: "A hovered comment card",
    rest: false,
    expectations: [
      "Hovered comment card fills with the same grey tint as a hovered sidebar row",
      "No drop shadow appears under the hovered card",
    ],
  });

  // An agent edits another file while the review is open.
  writeFileSync(
    join(alertingDir(), "packages/api/src/levels.ts"),
    'export const LEVELS = ["debug", "info", "warn", "error"] as const;\n',
  );
  await expect(page.getByRole("button", { name: "Reload" })).toBeVisible({ timeout: 15_000 });
  await capture(page, {
    name: "13-review-05-stale-banner",
    title: "An edit on disk during a review raises the stale banner",
    expectations: ["Amber banner: '1 file changed since you started reviewing', levels.ts, Reload"],
  });
  await page.getByRole("button", { name: "Reload" }).click();
  await settle(page, 1200);

  await page.getByRole("button", { name: "Apply suggestion" }).click();
  await settle(page, 2500);
  expect(readFileSync(join(alertingDir(), FILE), "utf8")).toContain("level.toUpperCase()");
  await capture(page, {
    name: "13-review-06-suggestion-applied",
    title: "Applying the suggestion rewrites the file and the diff follows",
    expectations: [
      "The comment is gone and the diff shows the toUpperCase line",
    ],
  });
});
