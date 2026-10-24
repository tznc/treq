// The Linear view's issues beside a preview of the one picked (TREQ-281):
// Kick off only in the preview, and issues created, edited and deleted
// there against the fake Linear (`fake-linear.ts`).
import { createHash } from "node:crypto";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { expect, test, type Page } from "@playwright/test";
import { fixture } from "../fixture.ts";
import { capture } from "./capture.ts";
import { ready, settle } from "./helpers.ts";

/** Gives the QA repository a Linear API key, so it reaches the fake
 * Linear; `null` takes it away. */
function linearKey(value: string | null): void {
  const { home, repo } = fixture();
  const hash = createHash("sha256").update(repo).digest("hex").slice(0, 16);
  const key = `repo_${hash}_linear_api_key`;
  const db = new DatabaseSync(join(home, ".local/share/com.treq/treq.db"));
  if (value === null) db.prepare("DELETE FROM settings WHERE key = ?").run(key);
  else
    db.prepare(
      "INSERT OR REPLACE INTO settings (key, value) VALUES (?, ?)",
    ).run(key, value);
  db.close();
}

test.describe.configure({ mode: "serial" });
test.beforeAll(() => linearKey("lin_api_qa"));
test.afterAll(() => linearKey(null));

async function linear(page: Page): Promise<void> {
  await ready(page);
  await page.getByRole("button", { name: "Linear", exact: true }).click();
  await expect(
    page.getByRole("button", { name: "ENG-142 Retry webhook deliveries" }),
  ).toBeVisible({ timeout: 20_000 });
  await settle(page, 800);
}

test("linear-issues: preview, create, edit and delete", async ({ page }) => {
  await linear(page);
  await expect(page.getByRole("button", { name: "Kick off" })).toHaveCount(0);
  await capture(page, {
    name: "22-linear-issues-01-list",
    title: "Linear issues beside an empty preview",
    expectations: [
      "Left: the issue list (ENG-142, ENG-139, ...) with no Kick off buttons on the rows",
      "Right: a preview pane reading 'Select an issue to preview it'",
      "A 'New issue' button with a plus icon sits left of the refresh button in the toolbar",
    ],
  });

  await page.getByRole("button", { name: "ENG-139 Cache key-value lookups" }).click();
  await expect(page.getByRole("button", { name: "Kick off" })).toHaveCount(1);
  await settle(page, 800);
  await capture(page, {
    name: "22-linear-issues-02-preview",
    title: "An issue picked: its preview",
    expectations: [
      "The ENG-139 row is highlighted in the list",
      "The preview shows ENG-139's badges, its title large, and edit (pencil), delete (trash) and 'Kick off' buttons at the top right",
      "Below: the description beside a COMMENTS column reading 'No comments'",
    ],
  });

  // Create.
  await page.getByRole("button", { name: "New issue" }).click();
  await page.getByRole("textbox", { name: "Title" }).fill("Alert on dead-letter growth");
  await page
    .getByRole("textbox", { name: "Description (optional, Markdown)" })
    .fill("Page when the DLQ grows by more than 100 in 5 minutes.");
  await settle(page, 500);
  await capture(page, {
    name: "22-linear-issues-03-new-form",
    title: "New issue form in the preview's place",
    expectations: [
      "The right pane shows 'New issue' with a Team select reading 'Engineering (ENG)'",
      "Title and description hold the typed text; 'Create issue' is enabled next to Cancel",
    ],
  });
  await page.getByRole("button", { name: "Create issue" }).click();
  await expect(
    page.getByRole("button", { name: "ENG-143 Alert on dead-letter growth" }),
  ).toBeVisible({ timeout: 20_000 });
  await settle(page, 800);
  await capture(page, {
    name: "22-linear-issues-04-created",
    title: "The created issue, listed first and previewed",
    expectations: [
      "ENG-143 'Alert on dead-letter growth' is the first, highlighted row",
      "The preview shows ENG-143 with its description and the Kick off button",
    ],
  });

  // Edit.
  await page.getByRole("button", { name: "Edit issue" }).click();
  const title = page.getByRole("textbox", { name: "Title" });
  await title.fill("Alert on dead-letter queue growth");
  await settle(page, 500);
  await capture(page, {
    name: "22-linear-issues-05-edit-form",
    title: "Editing the previewed issue",
    expectations: [
      "The preview swaps the title and description for an editor holding the new title and the description",
      "Save and Cancel under the editor; edit and delete buttons are hidden while editing",
    ],
  });
  await page.getByRole("button", { name: "Save" }).click();
  await expect(
    page.getByRole("button", { name: "ENG-143 Alert on dead-letter queue growth" }),
  ).toBeVisible({ timeout: 20_000 });
  await settle(page, 800);
  await capture(page, {
    name: "22-linear-issues-06-edited",
    title: "The edit saved",
    expectations: [
      "The list row and the preview title read 'Alert on dead-letter queue growth'",
    ],
  });

  // Delete.
  await page.getByRole("button", { name: "Delete issue" }).click();
  await expect(page.getByRole("button", { name: "Delete", exact: true })).toBeVisible();
  await settle(page, 500);
  await capture(page, {
    name: "22-linear-issues-07-delete-confirm",
    title: "Delete confirmation",
    expectations: [
      "A dialog 'Delete ENG-143?' says Linear moves it to its trash for 30 days",
      "Cancel and a red Delete button",
    ],
  });
  await page.getByRole("button", { name: "Delete", exact: true }).click();
  await expect(
    page.getByRole("button", { name: "ENG-143 Alert on dead-letter queue growth" }),
  ).toHaveCount(0, { timeout: 20_000 });
  await settle(page, 800);
  await capture(page, {
    name: "22-linear-issues-08-deleted",
    title: "The issue deleted",
    expectations: [
      "ENG-143 is gone from the list; ENG-142 is first again",
      "The preview is back to 'Select an issue to preview it'; no dialog",
    ],
  });
});

test("linear-issues: kanban board beside the preview", async ({ page }) => {
  await linear(page);
  // The List / Kanban switch has no accessible names; click it by place.
  await page.mouse.click(370, 143);
  await settle(page, 800);
  await capture(page, {
    name: "22-linear-issues-09-kanban",
    title: "Kanban board beside the preview",
    expectations: [
      "Columns by state (Backlog, Todo, In Progress, Done) on the left, with no Kick off buttons on the cards",
      "The board takes the full width; no preview pane until a card is picked",
    ],
  });
  await page.getByRole("button", { name: "ENG-125 Rate-limit the public API" }).click();
  await expect(page.getByRole("button", { name: "Kick off" })).toHaveCount(1);
  await settle(page, 800);
  await capture(page, {
    name: "22-linear-issues-10-kanban-preview",
    title: "Kanban board with an issue previewed",
    expectations: [
      "Columns by state on the left, with no Kick off buttons on the cards; the ENG-125 card is highlighted",
      "The right pane previews ENG-125 with its Kick off button",
    ],
  });
});
