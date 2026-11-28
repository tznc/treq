// The terminal pane's accent marks only the focused terminal, not every
// terminal of the shown workspace; and a click on the height divider leaves
// the pane's height alone, only a drag resizes it.
import { expect, test } from "@playwright/test";
import { capture } from "./capture.ts";
import { openTab, openWorkspace, row, settle } from "./helpers.ts";

const BRANCH = "feat/keyvalues-cache";

test("terminal-focus: the accent follows the focused terminal", async ({ page }) => {
  await openWorkspace(page, BRANCH);
  await openTab(page, "Commits");
  await row(page, BRANCH).hover();
  await row(page, BRANCH).getByRole("button", { name: "Open shell" }).click();
  await settle(page, 2500);
  await row(page, BRANCH).hover();
  await row(page, BRANCH).getByRole("button", { name: "Start agent" }).click();
  await settle(page, 3000);

  const shell = page.getByRole("group", { name: "Shell terminal" });
  const agent = page.getByRole("group", { name: /^Claude .* terminal$/ });
  await shell.click();
  await settle(page, 600);
  await expect(page.getByRole("group", { name: "Shell terminal", selected: true })).toBeVisible();
  await capture(page, {
    name: "33-terminal-focus-01-shell",
    title: "Shell focused, agent beside it in the same workspace",
    expectations: [
      "A blue accent runs along the top of the Shell column only",
      "The Claude column, of the same workspace, has no blue accent",
    ],
  });

  await agent.click();
  await settle(page, 600);
  await expect(page.getByRole("group", { name: /^Claude .* terminal$/, selected: true })).toBeVisible();
  await capture(page, {
    name: "33-terminal-focus-02-agent",
    title: "Agent focused",
    expectations: [
      "The blue accent moved to the top of the Claude column",
      "The Shell column has no blue accent",
    ],
  });

  await page.mouse.click(150, 800); // the sidebar's empty foot
  await settle(page, 600);
  await expect(page.getByRole("group", { selected: true })).toHaveCount(0);
  await capture(page, {
    name: "33-terminal-focus-03-none",
    title: "Focus outside the pane",
    expectations: ["No terminal column shows the blue accent"],
  });
});

test("terminal-divider: a click keeps the height, a drag resizes", async ({ page }) => {
  await openWorkspace(page, BRANCH);
  await openTab(page, "Commits");
  await row(page, BRANCH).hover();
  await row(page, BRANCH).getByRole("button", { name: "Open shell" }).click();
  await settle(page, 2500);

  const pane = page.getByRole("region", { name: "Terminal pane" });
  const before = await pane.boundingBox();
  if (!before) throw new Error("no terminal pane");
  await capture(page, {
    name: "33-terminal-divider-01-before",
    title: "Terminal pane at its default height",
    expectations: ["The pane takes about a third of the workspace view"],
  });

  // Press at the top of the divider's grab area, jitter a pixel, let go.
  const x = before.x + before.width / 2;
  await page.mouse.move(x, before.y - 7);
  await page.mouse.down();
  await page.mouse.move(x, before.y - 6);
  await page.mouse.up();
  await settle(page, 600);
  await capture(page, {
    name: "33-terminal-divider-02-clicked",
    title: "After a click on the height divider",
    expectations: ["The pane's top edge is exactly where it was before the click"],
  });
  // The edge follows the pointer's 1px jitter, no further.
  const clicked = await pane.boundingBox();
  expect(Math.abs((clicked?.y ?? 0) - before.y)).toBeLessThanOrEqual(1);

  const start = (await pane.boundingBox())?.y ?? before.y;
  await page.mouse.move(x, start - 2);
  await page.mouse.down();
  await page.mouse.move(x, start - 122, { steps: 10 });
  await page.mouse.up();
  await settle(page, 600);
  await capture(page, {
    name: "33-terminal-divider-03-dragged",
    title: "After dragging the divider up 120px",
    expectations: ["The pane grew taller, its top edge 120px higher than before"],
  });
  const dragged = await pane.boundingBox();
  expect(Math.abs((dragged?.y ?? 0) - (start - 120))).toBeLessThan(1);
});
