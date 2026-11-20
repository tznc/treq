// A picked Linear issue's branch follows the repository's branch name
// pattern: Linear's branch name fills `{name}`.
import { createHash } from "node:crypto";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { expect, test } from "@playwright/test";
import { fixture } from "../fixture.ts";
import { capture } from "./capture.ts";
import { home, settle } from "./helpers.ts";

function repoSetting(name: string, value: string | null): void {
  const { home, repo } = fixture();
  const hash = createHash("sha256").update(repo).digest("hex").slice(0, 16);
  const key = `repo_${hash}_${name}`;
  const db = new DatabaseSync(join(home, ".local/share/com.treq/treq.db"));
  if (value === null) db.prepare("DELETE FROM settings WHERE key = ?").run(key);
  else
    db.prepare("INSERT OR REPLACE INTO settings (key, value) VALUES (?, ?)").run(key, value);
  db.close();
}

test.describe.configure({ mode: "serial" });
test.afterAll(() => {
  repoSetting("linear_api_key", null);
  repoSetting("branch_name_pattern", null);
});

for (const [label, pattern, branch] of [
  ["default", null, "treq/octocateng-142-retry-webhook-deliveries"],
  ["no-prefix", "{name}", "octocateng-142-retry-webhook-deliveries"],
] as const) {
  test(`issue branch pattern: ${label}`, async ({ page }) => {
    repoSetting("linear_api_key", "lin_api_qa");
    repoSetting("branch_name_pattern", pattern);
    await home(page);
    await page.getByRole("button", { name: "New workspace…" }).first().click();
    await page.getByRole("tab", { name: "Linear" }).click();
    await expect(page.getByRole("heading", { name: "Your issues" })).toBeVisible({ timeout: 20_000 });
    await page.getByRole("checkbox", { name: "ENG-142 Retry webhook deliveries" }).click();
    await page.getByRole("status").click();
    const list = page.getByRole("list", { name: "Workspaces to open" });
    await expect(list).toBeVisible();
    await expect(list.getByText(branch, { exact: true })).toBeVisible();
    await settle(page, 500);
    await capture(page, {
      name: `32-issue-branch-pattern-${label}`,
      title: `Linear issue branch, pattern ${pattern ?? "treq/{name} (default)"}`,
      expectations: [`The popover lists ENG-142 as '${branch}', new branch`],
    });
    await page.keyboard.press("Escape");
    await page.keyboard.press("Escape");
  });
}
