// Sent review comments: a Changes-tab review sent to an agent leaves its
// comments on the diff, marked with a send icon, until resolved.
import { expect, test } from "@playwright/test";
import { capture } from "./capture.ts";
import { openTab, openWorkspace, settle } from "./helpers.ts";

const ALERTING = "feat/alerting-logs";

test("sent review comments stay on the diff until resolved", async ({ page }) => {
  await openWorkspace(page, ALERTING);
  await openTab(page, "Changes");
  await settle(page, 1500);

  // The first added line's gutter (by place in the 1400x900 window).
  await page.mouse.click(600, 277);
  await page.keyboard.press("ArrowLeft");
  const input = page.getByRole("textbox", { name: "Add a comment…" });
  await expect(input).toBeVisible();
  await input.fill("Log the level in upper case");
  await page.getByRole("button", { name: "Add comment", exact: true }).click();
  await expect(input).toHaveCount(0);

  await page.getByRole("button", { name: /^Finish review/ }).click();
  await page.getByRole("button", { name: "Plan", exact: true }).click();
  const resolve = page.getByRole("button", { name: "Resolve", exact: true });
  await expect(resolve).toBeVisible({ timeout: 15_000 });
  await expect(page.getByRole("button", { name: "Delete" })).toHaveCount(0);
  await settle(page, 800);
  await capture(page, {
    name: "26-sent-review-01-card",
    title: "A sent review comment stays on its line",
    expectations: [
      "Under the commented line a card shows a paper-plane icon and 'Sent to agent' in blue",
      "The card holds the comment text 'Log the level in upper case' and the file:line label",
      "Its only action is Resolve: no Send to agent, no Delete; the whole card is sans, not mono",
    ],
  });

  await resolve.click();
  await expect(resolve).toHaveCount(0, { timeout: 10_000 });
  const folded = page.getByRole("button", { name: "Show resolved comment" });
  await expect(folded).toBeVisible();
  await settle(page, 600);
  await capture(page, {
    name: "26-sent-review-02-resolved",
    title: "A resolved sent comment folds to one line",
    expectations: [
      "Under the commented line a one-line row shows a chevron, a paper-plane, 'Resolved' and the comment text, all muted",
      "No Resolve button and no full card",
    ],
  });

  await folded.click();
  await expect(page.getByRole("button", { name: "Hide resolved comment" })).toBeVisible();
  await settle(page, 600);
  await capture(page, {
    name: "26-sent-review-03-unfolded",
    title: "Clicking the folded row shows the comment again",
    expectations: [
      "The full sent card shows its text again, with 'Hide resolved comment' instead of Resolve",
    ],
  });
});
