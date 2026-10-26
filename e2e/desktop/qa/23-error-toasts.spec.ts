// An error toast that fires twice shows once, with a Copy button. The demo
// repo's origin is unreachable, so Push fails with "Sync failed".
import { test } from "@playwright/test";
import { capture } from "./capture.ts";
import { openWorkspace, settle } from "./helpers.ts";

test("error-toasts: a repeated push failure stacks once", async ({ page }) => {
  await openWorkspace(page, "feat/empty-event-message");
  const push = page.getByRole("button", { name: "Push", exact: true });
  await push.click();
  await settle(page, 4000);
  await push.click();
  await settle(page, 4000);
  await capture(page, {
    name: "23-error-toasts-01-deduped",
    title: "Push failed twice",
    screen: true,
    expectations: [
      "Exactly one 'Sync failed' error toast, not two stacked",
      "The toast has a copy icon button left of its close button",
    ],
  });
});
