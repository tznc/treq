// The header's PR status: one pill split button tinted in the PR state's
// GitHub color, the state's icon and label, then an open-on-web half. #401 is
// open and #388 merged.
import { expect, type Page, test } from "@playwright/test";
import { capture } from "./capture.ts";
import { openWorkspace, ready, settle } from "./helpers.ts";

test.describe.configure({ mode: "serial" });

/** The header's right group, around the PR button. */
async function headerClip(page: Page) {
  const box = await page.getByRole("button", { name: "Open PR on web" }).boundingBox();
  if (!box) throw new Error("no PR button");
  return { x: box.x + box.width - 360, y: box.y - 12, width: 372, height: box.height + 24 };
}

for (const [branch, state, color] of [
  ["feat/keyvalues-cache", "open", "green"],
  ["feat/alerting-logs", "merged", "purple"],
] as const) {
  test(`pr-status: ${state} PR button`, async ({ page }) => {
    await ready(page);
    await openWorkspace(page, branch);
    await settle(page, 1500);
    await expect(page.getByRole("button", { name: new RegExp(`^View PR .*${state}\\)$`) })).toBeVisible();
    await capture(page, {
      name: `16-pr-status-${state}`,
      title: `${state} PR button`,
      clip: await headerClip(page),
      expectations: [
        `The PR status is one fully rounded pill tinted ${color}, with a ${color} border`,
        `'${state[0].toUpperCase()}${state.slice(1)}' reads in ${color} beside the state's PR icon`,
        "The open-on-web half shares the tint, split by a 1px divider in the same tone",
      ],
    });
  });
}
