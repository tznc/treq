// A diff line's "Add comment" tooltip stays open while the pointer rests
// on the button: it used to hide and re-show every ~200ms, because the
// popup window opening sent a fresh hover-enter that reset it.
import { expect, test } from "@playwright/test";
import { capture } from "./capture.ts";
import {
  openTab,
  openWorkspace,
  popupBox,
  settle,
  xPointer,
} from "./helpers.ts";

test("diff add-comment tooltip does not flicker", async ({ page }) => {
  await openWorkspace(page, "feat/alerting-logs");
  await openTab(page, "Changes");
  await settle(page, 1500);

  const button = page
    .getByRole("button", { name: /^Add comment on .* line \d+$/ })
    .first();
  const box = await button.boundingBox();
  expect(box).not.toBeNull();
  await xPointer(page, box!.x + box!.width / 2, box!.y + box!.height / 2);

  // Sample the popup for 3s once it has opened: it must never close.
  await expect.poll(() => popupBox(), { timeout: 3000 }).not.toBeNull();
  const samples: boolean[] = [];
  for (let i = 0; i < 30; i++) {
    samples.push(popupBox() !== null);
    await page.waitForTimeout(100);
  }
  expect(samples.filter((s) => !s).length).toBe(0);

  await capture(page, {
    name: "27-tooltip-01-add-comment-steady",
    title: "Add comment tooltip after 3s of hover",
    screen: true,
    rest: false,
    expectations: [
      "An 'Add comment' tooltip shows next to the hovered diff line's + button",
      "The + button is visible (hover state) on that line",
    ],
  });
});
