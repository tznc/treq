// The Changes tab: the stacked workspace's conflict card and its Resolve
// popover, a committed file's diff with an inline comment added and
// edited, the review bar, the search bar, and the commit box with its
// split-button menu on a workspace with uncommitted edits.
//
// File rows and a diff line's comment button are not observable, so those
// steps click a fixed point (the landing capture's, for this seed and the
// 1400x900 window); each then waits on an observable result.
import { expect, test, type Page } from "@playwright/test";
import { capture } from "./capture.ts";
import { mod, openTab, openWorkspace, settle } from "./helpers.ts";

/** alerting.ts in feat/alerting-logs's Uncommitted list. */
const SELECTED_ROW = { x: 330, y: 283 };
/** The Uncommitted header's discard all button, for feat/alerting-logs. */
const DISCARD_ALL = { x: 453, y: 251 };

test.describe.configure({ mode: "serial" });

/** Comments on the diff line at `y` through its gutter's add button. */
async function commentOnLine(page: Page, y: number, text: string, shot?: string) {
  await page.mouse.move(800, y);
  await page.mouse.move(645, y, { steps: 4 });
  await page.mouse.click(645, y);
  const input = page.getByRole("textbox", { name: "Add a comment…" });
  await expect(input).toBeVisible();
  await input.fill(text);
  if (shot) {
    await capture(page, {
      name: shot,
      title: "Inline comment box on a diff line",
      expectations: [
        "Comment box opens under the clicked line, full width of the diff",
        "Cancel and Add Comment buttons aligned right",
      ],
    });
  }
  await page.getByRole("button", { name: "Add comment", exact: true }).click();
  await expect(input).toHaveCount(0);
}

test("changes: conflict card and resolve popover", async ({ page }) => {
  await openWorkspace(page, "feat/empty-event-message");
  await openTab(page, "Changes");
  await capture(page, {
    name: "05-changes-overview",
    title: "Changes tab: conflicts and committed files",
    expectations: [
      "File list: Conflicts (Home.tsx, C) above Committed 3 (A/A/M badges)",
      "Review bar: '0 comments pending', Start Review, Reset, red Resolve conflicts…",
      "Commit box (Message, Commit split button) at the top of the list",
    ],
  });
  await page.mouse.move(330, 315, { steps: 4 }); // Home.tsx under Conflicts
  await settle(page, 500);
  await capture(page, {
    name: "05-changes-file-row-hover",
    title: "Changes tab: a hovered file row",
    rest: false,
    expectations: [
      "Hovered Home.tsx row fills with the same grey tint as a hovered sidebar row",
      "No shadow or border appears around the hovered row",
    ],
  });
  await page.mouse.click(330, 283); // Home.tsx under Conflicts
  await settle(page, 1500);
  await capture(page, {
    name: "05-changes-conflict-card",
    title: "Changes tab: conflict card for Home.tsx",
    expectations: [
      "Both sides of the conflict shown with clear labels and colours",
      "Add comment / resolution note controls aligned",
    ],
  });
  const resolve = page.getByRole("button", { name: "Resolve conflicts…" });
  await resolve.click();
  await expect(page.getByRole("button", { name: "Plan" })).toBeVisible();
  await settle(page, 600);
  await capture(page, {
    name: "05-changes-resolve-popover",
    title: "Resolve conflicts popover (new commit by an agent)",
    expectations: [
      "Popover anchored under the button, not clipped by the window edge",
      "Plan / Edit actions and the agent choice readable",
    ],
  });
  await page.mouse.click(1362, 171); // the popover's close button
  await expect(page.getByRole("button", { name: "Plan" })).toHaveCount(0);
});

test("changes: diff, inline comment add/edit, review bar", async ({ page }) => {
  await openWorkspace(page, "feat/empty-event-message");
  await openTab(page, "Changes");
  await settle(page, 1500);
  await capture(page, {
    name: "05-changes-diff-top",
    title: "Changes tab: the diff list scrolled to the top",
    expectations: [
      "A gap between the first file card and the top of the diff section",
      "Diff code reads the same size as the 13px UI text beside it",
    ],
  });
  await page.mouse.click(326, 383); // client.ts under Committed
  await settle(page, 1500);
  await capture(page, {
    name: "05-changes-diff",
    title: "Changes tab: a committed file's diff",
    expectations: [
      "File header: path, copy icon, Committed badge, Viewed checkbox, +/- counts",
      "Added lines green, line numbers aligned, syntax colours readable",
      "A resolved GitHub review thread is folded on its line",
    ],
  });
  await commentOnLine(page, 364, "Prefer a typed guard over typeof here", "05-changes-comment-input");
  await settle(page, 800);
  await capture(page, {
    name: "05-changes-comment-added",
    title: "Inline comment added; review bar counts it",
    expectations: [
      "Comment card under line 6 with author and text",
      "Review bar reads '1 comment pending'",
    ],
  });
  // Clicking a comment's body edits it.
  await page.getByTestId(/^comment-/).first().click().catch(async () => {
    await page.mouse.click(800, 416);
  });
  await settle(page, 800);
  const edit = page.getByRole("button", { name: "Save" });
  if ((await edit.count()) > 0) {
    await capture(page, {
      name: "05-changes-comment-edit",
      title: "Editing an inline comment",
      expectations: ["Discard, Cancel and Save buttons; the text is editable in place"],
    });
    await page.getByRole("button", { name: "Cancel" }).first().click();
  }
  await page.mouse.click(571, 490); // unfold the resolved GitHub thread
  await settle(page, 800);
  await capture(page, {
    name: "05-changes-github-thread",
    title: "Resolved GitHub review thread unfolded",
    expectations: [
      "Thread shows author, body and Resolved state; actions aligned",
      "hubot's reply renders markdown: bold, inline code, a bulleted list and a link",
      "':robot:' shows as the robot emoji",
    ],
  });

  await page.getByRole("button", { name: "Start review" }).click();
  await settle(page, 1000);
  await capture(page, {
    name: "05-changes-review-started",
    title: "After Start review",
    screen: true,
    expectations: [
      "A 'Review started on feat/empty-event-message' toast reads 'Comments will appear inline'",
      "The AI Review terminal pane is open at a readable height, its column active",
      "Pending comment count still shown",
    ],
  });
  await page.keyboard.press("Escape");
  await settle(page, 600);
});

test("changes: search in diff", async ({ page }) => {
  await openWorkspace(page, "feat/empty-event-message");
  await openTab(page, "Changes");
  await page.mouse.click(900, 500); // focus the diff
  await page.keyboard.press(`${mod}+f`);
  await settle(page, 600);
  await page.keyboard.type("event_message");
  await settle(page, 1000);
  await capture(page, {
    name: "05-changes-search",
    title: "Search in the diff (Ctrl+F)",
    expectations: [
      "Search bar with the query, match count and next/previous controls",
      "Matches highlighted in the diff; the current one distinct",
    ],
  });
  await page.keyboard.press("Escape");
  await settle(page, 500);
});

test("changes: commit box and its split menu", async ({ page }) => {
  await openWorkspace(page, "feat/alerting-logs");
  await openTab(page, "Changes");
  await capture(page, {
    name: "05-changes-uncommitted",
    title: "Changes tab with uncommitted edits",
    expectations: [
      "Uncommitted section lists alerting.ts (M) and levels.ts (A) with checkboxes",
      "Commit box above; Commit disabled until a message is typed",
    ],
  });
  const message = page.getByRole("textbox", { name: "Message" });
  await message.fill("feat: add alert levels");
  await settle(page, 500);
  await capture(page, {
    name: "05-changes-commit-message",
    title: "Commit box with a message",
    expectations: ["Commit button enabled; message text not clipped"],
  });
  await page.getByRole("button", { name: "More commit options" }).click();
  await settle(page, 1000);
  await capture(page, {
    name: "05-changes-commit-split-menu",
    title: "Commit split-button menu",
    screen: true,
    rest: false,
    expectations: ["Commit variants (commit and push, amend, …) aligned, none clipped"],
  });
  await page.keyboard.press("Escape");
  await message.fill("");
});

test("changes: selected file row keeps its actions", async ({ page }) => {
  await openWorkspace(page, "feat/alerting-logs");
  await openTab(page, "Changes");
  await page.mouse.click(SELECTED_ROW.x, SELECTED_ROW.y); // alerting.ts under Uncommitted
  await page.mouse.move(900, 600); // off the row: no hover state
  await settle(page, 800);
  await capture(page, {
    name: "05-changes-selected-row",
    title: "Selected file row without hover",
    expectations: [
      "Selected alerting.ts row shows status M, discard and + without hovering",
      "Status letter and action icons readable on the blue selection",
    ],
  });
});

test("changes: Escape closes the discard all confirmation", async ({ page }) => {
  await openWorkspace(page, "feat/alerting-logs");
  await openTab(page, "Changes");
  await page.mouse.click(DISCARD_ALL.x, DISCARD_ALL.y); // Uncommitted's discard all
  const confirm = page.getByRole("button", { name: "Discard all changes", exact: true });
  await expect(confirm).toBeVisible();
  await settle(page, 500);
  await capture(page, {
    name: "05-changes-discard-all-open",
    title: "Discard all changes? confirmation",
    expectations: ["Dialog centred over a dimmed window with Cancel and Discard all changes"],
  });
  await page.keyboard.press("Escape");
  await settle(page, 500);
  await capture(page, {
    name: "05-changes-discard-all-escaped",
    title: "After Escape on the confirmation",
    expectations: [
      "Dialog closed; the Changes tab shows again",
      "alerting.ts and levels.ts still listed under Uncommitted",
    ],
  });
  await expect(confirm).toHaveCount(0);
});

test("changes: generated files wait behind Load diff", async ({ page }) => {
  await openWorkspace(page, "feat/event-ingest");
  await openTab(page, "Changes");
  const load = page.getByRole("button", { name: "Load diff" });
  await expect(load).toHaveCount(2);
  await capture(page, {
    name: "05-changes-generated-deferred",
    title: "Generated files' diffs wait behind Load diff",
    expectations: [
      "package-lock.json and generated/schema.ts cards say the file is generated, with a Load diff button",
      "retry.ts and .gitattributes show their diffs as usual",
    ],
  });
  await load.first().click();
  await expect(load).toHaveCount(1);
  await settle(page, 800);
  await capture(page, {
    name: "05-changes-generated-loaded",
    title: "A generated file's diff after Load diff",
    expectations: ["The clicked generated file's diff renders in place of the placeholder"],
  });
});

test("changes: filter menu picks what the lists show", async ({ page }) => {
  await openWorkspace(page, "feat/event-ingest");
  await openTab(page, "Changes");
  await expect(page.getByRole("button", { name: "Show committed changes" })).toHaveCount(0);
  await capture(page, {
    name: "05-changes-filter-row",
    title: "The list's filter row",
    expectations: [
      "The funnel button sits in its own right-aligned row above the lists",
      "The Committed header has no Show button",
    ],
  });
  await page.getByRole("button", { name: "Filter changes" }).click();
  await settle(page, 1000);
  await capture(page, {
    name: "05-changes-filter-menu",
    title: "The list's filter menu",
    screen: true,
    rest: false,
    expectations: ["The menu lists Generated files and Committed, both checked"],
  });
  await page.keyboard.press("Escape");
  await settle(page, 500);
});
