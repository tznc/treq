// A local review comment the review agent left (tagged "Local") and its
// actions: Send to agent, Resolve and Delete, drawn in the UI font like every
// other button, Delete not greyed as if disabled. The spec sends a review to
// get a comment on the diff, then copies it as the agent's own into the
// repository's `.treq/local.db`.
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { expect, test } from "@playwright/test";
import { fixture } from "../fixture.ts";
import { capture } from "./capture.ts";
import { openTab, openWorkspace, settle } from "./helpers.ts";

/** Copies the sent comment `text` as a local agent comment on `line`. */
function addAgentComment(text: string, line: number, comment: string): void {
  const db = new DatabaseSync(join(fixture().repo, ".treq", "local.db"));
  db.prepare(
    `INSERT INTO agent_review_comments (id, repo_path, target_type, target_id, file_path, hunk_id,
       start_line, end_line, side, comment_text, suggested_replacement, status, source, created_at)
     SELECT 'qa-local-agent', repo_path, target_type, target_id, file_path, hunk_id, ?, ?, side, ?,
       NULL, 'open', 'local-agent', created_at
     FROM agent_review_comments WHERE comment_text = ? LIMIT 1`,
  ).run(line, line, comment, text);
  db.close();
}

test("local-review-comment-actions: Send to agent, Resolve, Delete", async ({ page }) => {
  await openWorkspace(page, "feat/alerting-logs");
  await openTab(page, "Changes");
  await settle(page, 1500);
  // Line 3's gutter selects the line; Left opens a comment box on it.
  await page.getByRole("button", { name: "Line 3 of packages/api/src/alerting.ts" }).click();
  await page.keyboard.press("ArrowLeft");
  await page.getByRole("textbox", { name: "Add a comment…" }).fill("Log the level in upper case");
  await page.getByRole("button", { name: "Add comment", exact: true }).click();
  await page.getByRole("button", { name: /^Finish review/ }).click();
  await page.getByRole("button", { name: "Plan", exact: true }).click();
  await expect(page.getByRole("button", { name: "Resolve", exact: true })).toBeVisible({
    timeout: 15_000,
  });

  addAgentComment("Log the level in upper case", 6, "Pass the level through formatAlert instead.");
  // The agent's comments are read again every 10 seconds.
  await expect(page.getByRole("button", { name: "Send to agent" })).toBeVisible({
    timeout: 15_000,
  });
  await expect(page.getByRole("button", { name: "Delete" })).toBeVisible();
  // The review's agent pane covers the bottom of the diff: scroll the card above it.
  await page.getByRole("group", { name: /^Sent comment on/ }).hover();
  await page.mouse.wheel(0, 240);
  await settle(page, 800);
  await capture(page, {
    name: "29-local-review-comment-actions-01-card",
    title: "A local review comment's actions",
    expectations: [
      "A violet 'Local' card holds 'Pass the level through formatAlert instead.' and the buttons Send to agent, Resolve and Delete",
      "The button labels are in the UI's sans font, not the card's monospace text",
      "Delete reads as an enabled ghost button, not greyed out",
    ],
  });
});
