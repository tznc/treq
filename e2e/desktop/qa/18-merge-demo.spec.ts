// Video demo of the local merge flow: open the stacked parent
// feat/event-ingest, open the Merge Preview, confirm the merge, and watch
// its child feat/empty-event-message move onto main in the sidebar
// (TREQ-257 retargets stacked children when their parent lands).
import { expect, test } from "@playwright/test";
import { STACK_PARENT, MARKETING } from "../landing/seed.ts";
import { capture } from "./capture.ts";
import { openWorkspace, row, settle } from "./helpers.ts";
import { mkdirSync } from "node:fs";
import { join } from "node:path";
import type { Page } from "@playwright/test";

/** With DEMO_FRAMES=<dir>, screenshot the window continuously so the run
 * can be stitched into a video (x11grab sees nothing of GPUI's surface). */
function recordFrames(page: Page): () => Promise<void> {
  const dir = process.env.DEMO_FRAMES;
  if (!dir) return async () => {};
  mkdirSync(dir, { recursive: true });
  let n = 0;
  let stop = false;
  const loop = (async () => {
    while (!stop) {
      const path = join(dir, `${String(n).padStart(5, "0")}.png`);
      try {
        await page.screenshot({ path });
        n++;
      } catch {
        // The window may be mid-redraw; take the next frame.
      }
      await new Promise((r) => setTimeout(r, 150));
    }
  })();
  return async () => {
    stop = true;
    await loop;
  };
}

test("merge demo: merging a stacked parent retargets its child", async ({ page }) => {
  const stopFrames = recordFrames(page);
  await openWorkspace(page, STACK_PARENT);
  await settle(page, 2500);
  await capture(page, {
    name: "18-merge-demo-01-stack",
    title: "Stacked parent selected; its child sits under it",
    expectations: [
      `${MARKETING} is indented under ${STACK_PARENT} in the sidebar`,
      "The workspace header shows Merge…",
    ],
  });

  await page.getByRole("button", { name: "Merge…" }).click();
  await expect(page.getByRole("button", { name: "Back" })).toBeVisible();
  await settle(page, 3000);
  await capture(page, {
    name: "18-merge-demo-02-preview",
    title: `Merge Preview: ${STACK_PARENT} into main`,
    expectations: ["Commit message, Confirm merge, commits and files to merge"],
  });

  await page.getByRole("button", { name: "Confirm merge" }).click();
  await expect(row(page, STACK_PARENT)).toHaveCount(0, { timeout: 30_000 });
  await settle(page, 3000);
  await capture(page, {
    name: "18-merge-demo-03-merged",
    title: "After the merge",
    expectations: [
      `${STACK_PARENT} is gone from the sidebar`,
      `${MARKETING} is now a top-level workspace on main`,
    ],
  });

  await openWorkspace(page, MARKETING);
  await settle(page, 3000);
  await capture(page, {
    name: "18-merge-demo-04-child",
    title: "The child now targets main",
    expectations: [`${MARKETING} → main in the header`],
  });
  await stopFrames();
});
