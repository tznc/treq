// GitHub review suggestions in the Changes tab: a thread whose comment holds
// a ```suggestion block, from the fake `gh` (PR #418).
import { expect, test } from "@playwright/test";
import { capture } from "./capture.ts";
import { openTab, openWorkspace, settle } from "./helpers.ts";

test.describe.configure({ mode: "serial" });

test("apply-suggestion: github thread", async ({ page }) => {
  await openWorkspace(page, "feat/empty-event-message");
  await openTab(page, "Changes");
  await settle(page, 1500);
  await page.getByRole("button", { name: "packages/web/src/lib/client.ts", exact: true }).click();
  await settle(page, 1500);
  await expect(page.getByText("Trim before checking length:")).toBeVisible();
  const commit = page.getByRole("button", { name: "Apply & commit", exact: true });
  await expect(page.getByRole("button", { name: "Apply", exact: true })).toBeEnabled();
  await expect(commit).toBeEnabled();
  await capture(page, {
    name: "21-apply-suggestion-01-thread",
    title: "Apply buttons on the reaction row",
    expectations: [
      "zoe's suggestion shows Apply & commit, then the primary Apply rightmost, on the add-reaction line",
      "Resolve sits under the comment; the thread is unresolved",
    ],
  });

  await commit.click();
  await settle(page, 2000);
  await expect(page.getByRole("button", { name: "Apply & commit", exact: true })).toHaveCount(0);
  await capture(page, {
    name: "21-apply-suggestion-02-committed",
    title: "Committed and resolved",
    screen: true,
    expectations: [
      "zoe's thread is folded with a Resolved pill",
      "A 'Suggestion committed' toast shows",
    ],
  });

  await openTab(page, "Commits");
  await settle(page, 1500);
  await expect(page.getByText("Apply suggestion from @zoe").first()).toBeVisible();
  await capture(page, {
    name: "21-apply-suggestion-03-commit",
    title: "The new commit",
    expectations: ["The newest commit reads 'Apply suggestion from @zoe'"],
  });
});
