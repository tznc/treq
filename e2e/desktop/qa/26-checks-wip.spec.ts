// The GitHub WIP app's status never completes. The Checks popover lists it
// last, muted and labelled inactive; the chip's count and spinner leave it
// out. feat/keyvalues-cache has three passing checks plus WIP.
import { expect, test } from "@playwright/test";
import { capture } from "./capture.ts";
import { dismissPopover, openWorkspace, ready, settle } from "./helpers.ts";

test("checks-wip: WIP status is inactive, not counted", async ({ page }) => {
  await ready(page);
  await openWorkspace(page, "feat/keyvalues-cache");
  await settle(page, 1500);
  const chip = page.getByRole("button", { name: /^Checks:/ });
  await expect(chip).toHaveAccessibleName(/3\/3/);
  await chip.click();
  await settle(page, 1200);
  await capture(page, {
    name: "26-checks-wip-popover",
    title: "Checks popover with the WIP status",
    screen: true,
    rest: false,
    expectations: [
      "The header reads GitHub checks · 3/3 and the chip shows a green check, not a spinner",
      "WIP is the last row: a grey dash icon, grey name and 'inactive' where a duration would be",
    ],
  });
  await dismissPopover(page);
});
