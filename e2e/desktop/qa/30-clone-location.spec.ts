// Clone directory: the Application tab's field with its Browse button, and
// the Projects tab listing each saved project with its own folder (empty
// follows Application). Clones land in <folder>/<owner>/<repo>.
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { expect, test, type Page } from "@playwright/test";
import { fixture } from "../fixture.ts";
import { capture } from "./capture.ts";
import { ready, settle } from "./helpers.ts";

test.describe.configure({ mode: "serial" });

const appDb = () => new DatabaseSync(join(fixture().home, ".local/share/com.treq/treq.db"));

/** Two saved projects in the running app's database, one with its own folder. */
function seedProjects(): void {
  const { repo } = fixture();
  const db = appDb();
  const at = new Date().toISOString();
  const add = db.prepare(
    "INSERT OR REPLACE INTO projects (id, name, created_at, last_opened_at, clone_directory) VALUES (?, ?, ?, ?, ?)",
  );
  add.run("qa-checkout", "Checkout", at, at, "~/work");
  add.run("qa-billing", "Billing", at, at, "");
  const repos = db.prepare("INSERT OR IGNORE INTO project_repos VALUES (?, ?, 0)");
  repos.run("qa-checkout", repo);
  repos.run("qa-billing", `${repo}-billing`);
  db.close();
}

async function settings(page: Page, tab: string): Promise<void> {
  await ready(page);
  const back = page.getByRole("button", { name: "Back", exact: true });
  if ((await back.count()) === 0) {
    await page.getByRole("button", { name: "Settings", exact: true }).click();
    await expect(back).toBeVisible();
  }
  await page.getByRole("tab", { name: tab }).click();
  await expect(page.getByRole("tab", { name: tab, selected: true })).toBeVisible();
  await page.mouse.move(1000, 860);
  await settle(page, 1200);
}

test("clone directory: application field", async ({ page }) => {
  await settings(page, "Application");
  await page.mouse.move(950, 500);
  await page.mouse.wheel(0, 4000);
  await settle(page, 900);
  await expect(page.getByRole("textbox", { name: "~/.treq/repos" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Browse for clone directory" })).toBeVisible();
  await capture(page, {
    name: "30-clone-location-01-application",
    title: "Settings: Application clone directory",
    expectations: [
      "A 'Clone directory' field at the end of the Application tab, ~/.treq/repos as placeholder",
      "A folder icon button right of the field, aligned with it",
      "Help text says clones go in <owner>/<repo> folders and projects can choose their own",
    ],
  });
});

test("clone directory: projects tab", async ({ page }) => {
  seedProjects();
  await settings(page, "Projects");
  await expect(page.getByRole("textbox")).toHaveCount(2);
  await expect(page.getByRole("button", { name: "Browse for Billing clone directory" })).toBeVisible();
  await capture(page, {
    name: "30-clone-location-02-projects-tab",
    title: "Settings: Projects tab",
    expectations: [
      "A Projects entry in the settings nav, selected",
      "Billing (first) is empty with ~/.treq/repos as placeholder; Checkout shows ~/work",
      "Each row has a folder icon button beside its field",
    ],
  });
  // Same last-opened time, so the list sorts by name: Billing first.
  await page.getByRole("textbox").first().fill("~/billing");
  await settle(page, 900);
  await capture(page, {
    name: "30-clone-location-03-project-saved",
    title: "Settings: a project folder saved itself",
    expectations: [
      "Billing reads ~/billing and shows the Saved tick by its label",
      "Checkout still reads ~/work",
    ],
  });
  const db = appDb();
  const dir = (id: string) =>
    (db.prepare("SELECT clone_directory AS d FROM projects WHERE id = ?").get(id) as { d: string }).d;
  const [billing, checkout] = [dir("qa-billing"), dir("qa-checkout")];
  db.close();
  expect(billing).toBe("~/billing");
  expect(checkout).toBe("~/work");
});
