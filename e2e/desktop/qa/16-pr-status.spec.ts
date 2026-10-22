// The header's PR status button: filled in the PR state's GitHub color, with
// bold white text and the state's PR icon, as a pill. #401 is open and #388
// merged.
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
        `The PR button group is filled ${color}, as a pill, with no outline look`,
        `'${state[0].toUpperCase()}${state.slice(1)}' reads in bold white beside the state's PR icon`,
        "The web button shares the fill, split by a faint white divider",
      ],
    });
  });
}
