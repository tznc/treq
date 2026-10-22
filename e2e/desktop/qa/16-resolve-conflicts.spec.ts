// The Commits tab's Resolve conflicts dialog, end to end: typed instructions
// enable Resolve, and submitting closes the dialog.
// Runs last: it leaves a resolve workspace and an agent session behind.
import { expect, test } from "@playwright/test";
import { capture } from "./capture.ts";
import { openTab, openWorkspace, settle } from "./helpers.ts";

test.describe.configure({ mode: "serial" });

test("resolve-conflicts: typed instructions start a resolve agent", async ({ page }) => {
  await openWorkspace(page, "feat/empty-event-message");
  await openTab(page, "Commits");
  await settle(page, 1500);
  await page.getByRole("button", { name: "Resolve conflicts…" }).click();
  const instructions = page.getByRole("textbox", {
    name: /^e\.g\. Prefer the workspace-side changes/,
  });
  await expect(instructions).toBeVisible();
  await instructions.fill("Keep both sides of the event feed");
  const resolve = page.getByRole("button", { name: "Resolve", exact: true });
  await expect(resolve).toBeEnabled({ timeout: 15_000 });
  await settle(page, 600);
  await capture(page, {
    name: "16-resolve-conflicts-ready",
    title: "Resolve conflicts dialog ready to submit",
    screen: true,
    expectations: [
      "Typed instructions show in the field",
      "Resolve is enabled (primary colour, not dimmed)",
      "No error text under the field",
    ],
  });

  await resolve.click();
  await expect(instructions).toHaveCount(0, { timeout: 15_000 });
});
