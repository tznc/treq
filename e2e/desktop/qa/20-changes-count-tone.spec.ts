// The Changes tab's count: yellow when the workspace has uncommitted
// changes, neutral when everything is committed.
import { expect, test } from "@playwright/test";
import { capture } from "./capture.ts";
import { openWorkspace } from "./helpers.ts";

test.describe.configure({ mode: "serial" });

test("changes count: uncommitted is yellow", async ({ page }) => {
  await openWorkspace(page, "feat/alerting-logs");
  await expect(page.getByRole("img", { name: /uncommitted changes$/ })).toBeVisible();
  await capture(page, {
    name: "20-changes-count-uncommitted",
    title: "Changes tab count with uncommitted changes",
    expectations: [
      "The count on the Changes tab is a yellow pill (yellow text on a yellow tint)",
      "The Commits count stays a neutral grey pill",
    ],
  });
});

test("changes count: committed only stays neutral", async ({ page }) => {
  await openWorkspace(page, "feat/empty-event-message");
  await expect(page.getByRole("img", { name: /uncommitted changes$/ })).toHaveCount(0);
  await capture(page, {
    name: "20-changes-count-committed",
    title: "Changes tab count with nothing uncommitted (conflicted)",
    expectations: ["The Changes count is a neutral grey pill, not yellow", "A red conflict dot follows the count"],
  });
});
