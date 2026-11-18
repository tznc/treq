// Resolving a GitHub review thread from the Changes tab (fake `gh`, PR #418).
import { expect, test } from "@playwright/test";
import { capture } from "./capture.ts";
import { openTab, openWorkspace, settle } from "./helpers.ts";

test("resolve-thread: github thread", async ({ page }) => {
  await openWorkspace(page, "feat/empty-event-message");
  await openTab(page, "Changes");
  await settle(page, 1500);
  await page.getByRole("button", { name: "packages/web/src/lib/client.ts", exact: true }).click();
  await settle(page, 1500);
  const resolve = page.getByRole("button", { name: "Resolve", exact: true });
  await expect(resolve).toHaveCount(1); // the octocat thread is already resolved
  await capture(page, {
    name: "21-resolve-thread-01-open",
    title: "An open thread offers Resolve",
    expectations: [
      "zoe's open thread has a Resolve button under its comment",
      "octocat's resolved thread is folded and has no Resolve button",
    ],
  });
  await resolve.click();
  await settle(page, 800);
  await expect(page.getByRole("button", { name: "Resolve", exact: true })).toHaveCount(0);
  await capture(page, {
    name: "21-resolve-thread-02-resolved",
    title: "Resolved",
    expectations: ["zoe's thread is folded with a Resolved pill, like octocat's"],
  });
});
