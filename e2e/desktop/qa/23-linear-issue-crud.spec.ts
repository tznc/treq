// Linear issues created, edited and deleted from the Linear view (TREQ-281)
// against the fake Linear (./fake-linear.ts): "New issue" in the toolbar
// opens the form, and an issue's page holds Edit and Delete beside the
// agent button.
import { createHash } from "node:crypto";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { expect, test, type Page } from "@playwright/test";
import { fixture } from "../fixture.ts";
import { capture } from "./capture.ts";
import { ready, settle } from "./helpers.ts";

test.describe.configure({ mode: "serial" });

/** Sets the QA repository's Linear API key (`Database::make_repo_key`);
 * `null` removes it. */
function linearKey(value: string | null): void {
  const { home, repo } = fixture();
  const hash = createHash("sha256").update(repo).digest("hex").slice(0, 16);
  const key = `repo_${hash}_linear_api_key`;
  const db = new DatabaseSync(join(home, ".local/share/com.treq/treq.db"));
  if (value === null) db.prepare("DELETE FROM settings WHERE key = ?").run(key);
  else db.prepare("INSERT OR REPLACE INTO settings (key, value) VALUES (?, ?)").run(key, value);
  db.close();
}

test.beforeAll(() => linearKey("lin_api_qa"));
test.afterAll(() => linearKey(null));

async function linear(page: Page): Promise<void> {
  await ready(page);
  const close = page.getByRole("button", { name: "Back", exact: true });
  if ((await close.count()) > 0) await close.first().click();
  await page.getByRole("button", { name: "Linear", exact: true }).click();
  await expect(page.getByRole("listitem", { name: "Retry webhook deliveries" })).toBeVisible({
    timeout: 20_000,
  });
  await page.mouse.move(1000, 860);
  await settle(page, 1000);
}

test("linear-issue-crud: create, edit and delete an issue", async ({ page }) => {
  await linear(page);
  await page.getByRole("button", { name: "New issue" }).click();
  await page.getByRole("textbox", { name: "Title" }).fill("Alert on dead-letter growth");
  await page
    .getByRole("textbox", { name: "Description (optional, Markdown)" })
    .fill("Page when the DLQ grows by more than 100 in 5 minutes.");
  await settle(page, 500);
  await capture(page, {
    name: "23-linear-crud-01-new-form",
    title: "Linear: New issue form",
    expectations: [
      "The list gives way to a 'New issue' page with a Back button and a Team select reading 'Engineering (ENG)'",
      "Title and description hold the typed text; 'Create issue' is enabled next to Cancel",
    ],
  });

  await page.getByRole("button", { name: "Create issue" }).click();
  await expect(page.getByRole("button", { name: "Edit issue" })).toBeVisible({ timeout: 20_000 });
  await settle(page, 1000);
  await capture(page, {
    name: "23-linear-crud-02-created",
    title: "Linear: the created issue's page",
    expectations: [
      "ENG-144's page: Back, the identifier, edit (pencil), delete (trash), the agent button and Open in Linear",
      "Title 'Alert on dead-letter growth', a Todo badge, and the typed description",
    ],
  });

  await page.getByRole("button", { name: "Edit issue" }).click();
  await page.getByRole("textbox", { name: "Title" }).fill("Alert on dead-letter queue growth");
  await settle(page, 500);
  await capture(page, {
    name: "23-linear-crud-03-edit-form",
    title: "Linear: editing an issue",
    expectations: [
      "The page swaps its title and description for an editor holding the new title and the description",
      "Save and Cancel under the editor; edit and delete are hidden while editing",
    ],
  });
  await page.getByRole("button", { name: "Save" }).click();
  await expect(page.getByRole("button", { name: "Edit issue" })).toBeVisible({ timeout: 20_000 });
  await settle(page, 1000);
  await capture(page, {
    name: "23-linear-crud-04-edited",
    title: "Linear: the edit saved",
    expectations: ["The page title reads 'Alert on dead-letter queue growth'; the editor is closed"],
  });

  await page.getByRole("button", { name: "Delete issue" }).click();
  await expect(page.getByRole("button", { name: "Delete", exact: true })).toBeVisible();
  await settle(page, 500);
  await capture(page, {
    name: "23-linear-crud-05-delete-confirm",
    title: "Linear: delete confirmation",
    expectations: [
      "A dialog 'Delete ENG-144?' says Linear moves it to its trash for 30 days",
      "Delete is the filled red destructive variant, with a light label",
      "Only one filled button in the dialog: Cancel is a ghost button",
    ],
  });
  await page.getByRole("button", { name: "Delete", exact: true }).click();
  await expect(page.getByRole("button", { name: "Back to issues" })).toHaveCount(0, {
    timeout: 20_000,
  });
  await expect(
    page.getByRole("listitem", { name: "Alert on dead-letter queue growth" }),
  ).toHaveCount(0);
  await settle(page, 1000);
  await capture(page, {
    name: "23-linear-crud-06-deleted",
    title: "Linear: the issue deleted",
    expectations: [
      "Back on the issue list; 'Alert on dead-letter queue growth' is gone and ENG-142 is first",
      "No dialog",
    ],
  });
});
