// A Markdown `treq send` attachment opens rendered in the lightbox, with
// its line comments in the Source tab.
import { test } from "@playwright/test";
import { appBinary } from "../env.ts";
import { capture } from "./capture.ts";
import { openTab, openWorkspace, row, settle } from "./helpers.ts";

const BRANCH = "feat/keyvalues-cache";

const DOC = [
  "# Handoff: cache invalidation",
  "",
  "## Symptom",
  "",
  "- Stale reads after `PUT /keys/:id`",
  "- [x] Reproduced locally",
  "- [ ] Fix the TTL",
  "",
  "> The cache keeps the old value for one TTL.",
  "",
  "```ts",
  "cache.delete(key);",
  "```",
  "",
  "| Step | Result |",
  "| --- | --- |",
  "| PUT | 200 |",
  "",
  "See [the notes](notes.md).",
].join("\n");

test("send markdown: preview and markdown tabs with comments", async ({ page }) => {
  await openWorkspace(page, BRANCH);
  await openTab(page, "Commits");
  await row(page, BRANCH).hover();
  await row(page, BRANCH).getByRole("button", { name: "Open shell" }).click();
  await settle(page, 2500);
  await page.mouse.click(1000, 780);
  await page.keyboard.type(`PS1='$ '; cat > HANDOFF.md <<'EOF'\n${DOC}\nEOF\n`);
  await page.keyboard.type("cp HANDOFF.md treq-force-rebase-handoff.md; echo hi > notes.txt\n");
  await page.keyboard.type(
    `for f in HANDOFF.md treq-force-rebase-handoff.md notes.txt; do '${appBinary}' send $f; done; clear\n`,
  );
  await settle(page, 1500);
  await capture(page, {
    name: "26-send-markdown-cards",
    title: "Attachment cards: Markdown icon, end-truncated names",
    expectations: [
      "The two .md cards show a Markdown (M with a down arrow) icon; notes.txt keeps the plain file icon",
      "treq-force-rebase-handoff.md shows its start and ends in an ellipsis, not clipped at both ends",
    ],
  });
  // The cards draw their names, which Playwright cannot query.
  await page.mouse.click(364, 693);
  await settle(page, 1000);
  await capture(page, {
    name: "26-send-markdown-rendered",
    title: "Lightbox: Markdown rendered",
    expectations: [
      "The handoff reads as formatted Markdown: headings, a bullet list, task checkboxes, a quote, a highlighted code block and a table",
      "A Preview | Markdown segmented control sits at the top right, Preview selected",
      "No comments yet, so no margin: the document takes the full width",
    ],
  });
  // The bullet list block; rendered blocks draw no accessible text.
  await page.mouse.click(700, 441);
  await settle(page, 300);
  await page.keyboard.type("Add the PUT repro steps");
  await capture(page, {
    name: "26-send-markdown-preview-draft",
    title: "Preview: writing a comment in the margin",
    expectations: [
      "The comment box is a card in the right-hand comments column, level with the list, no wider than the column",
      "Nothing is inserted into the document: no line or box cuts across the preview",
    ],
  });
  await page.getByRole("button", { name: "Add comment" }).click();
  await settle(page, 500);
  await capture(page, {
    name: "26-send-markdown-block-comment",
    title: "Preview: a comment on a block",
    expectations: [
      "The bullet/task list block is tinted as commented",
      "A comment card in the right margin, level with the list, reads 'Lines 5–7' and 'Add the PUT repro steps'",
    ],
  });
  await page.getByText("Markdown", { exact: true }).click();
  await settle(page, 500);
  // Line 9, the quote.
  await page.mouse.click(500, Number(process.env.QA_LINE_Y ?? 486));
  await settle(page, 300);
  await page.getByRole("button", { name: "Suggest change" }).click();
  await settle(page, 300);
  await capture(page, {
    name: "26-send-markdown-suggest-draft",
    title: "Markdown tab: Suggest change in the comment box",
    expectations: [
      "The inline comment box under line 9 holds a ```suggestion block with the quote's source line",
      "A 'Suggest change' button sits at the left of the box's buttons, Cancel and Add comment at the right",
    ],
  });
  await page.keyboard.press("Control+End");
  await page.keyboard.type("Say how long the TTL is");
  await page.getByRole("button", { name: "Add comment" }).click();
  await settle(page, 500);
  await capture(page, {
    name: "26-send-markdown-source",
    title: "Markdown tab: the block's comment and a line comment",
    expectations: [
      "The raw Markdown shows in monospace with line numbers and the 'Click a line to comment' hint",
      "Lines 5–7 are tinted; the Preview's comment card sits in the right margin beside line 5, and the line comment beside its line",
    ],
  });
  await page.getByText("Preview", { exact: true }).click();
  await settle(page, 500);
  await capture(page, {
    name: "26-send-markdown-both-comments",
    title: "Preview: both comments in one review",
    expectations: [
      "Both comment cards show in the right margin beside the blocks they annotate",
      "The quote's card shows a 'Suggested change' diff and no Apply button",
    ],
  });
});
