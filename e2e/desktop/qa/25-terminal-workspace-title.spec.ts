// An agent column's workspace chip shows the workspace's title, not its
// branch, and follows the title when it is edited on the Code tab. A title
// too long for the chip ends in an ellipsis; hovering shows it whole.
import { expect, test } from "@playwright/test";
import { capture } from "./capture.ts";
import { mod, openTab, openWorkspace, row, settle } from "./helpers.ts";

const BRANCH = "feat/empty-event-message";
const TITLE = "Handle empty event messages without dropping the whole batch";

test.describe.configure({ mode: "serial" });

test("terminal workspace title: the agent chip names the workspace's title", async ({ page }) => {
  await openWorkspace(page, BRANCH);
  await row(page, BRANCH).hover();
  await row(page, BRANCH).getByRole("button", { name: "Start agent" }).click();
  await settle(page, 3000);
  await capture(page, {
    name: "25-terminal-workspace-title-branch",
    title: "Agent header before the workspace has a title",
    clip: { x: 0, y: 560, width: 1400, height: 110 },
    expectations: ["The agent header's chip reads the branch, feat/empty-event-message"],
  });

  // Title the workspace on the Code tab, with the pane collapsed so the
  // title shows; the open agent's chip follows.
  await page.getByRole("button", { name: "Collapse" }).click();
  await openTab(page, "Code");
  await settle(page, 1500);
  await page.getByRole("button", { name: "Edit workspace title" }).click();
  await settle(page, 400);
  await page.keyboard.press(`${mod}+a`);
  await page.keyboard.type(TITLE);
  await page.getByRole("button", { name: "Save", exact: true }).click();
  await settle(page, 2000);
  await page.getByRole("button", { name: "Expand" }).click();
  await settle(page, 1000);

  await expect(page.getByRole("group", { name: "Claude 1 terminal" })).toBeVisible();
  await capture(page, {
    name: "25-terminal-workspace-title-chip",
    title: "Agent header after the workspace is titled",
    clip: { x: 0, y: 560, width: 1400, height: 110 },
    expectations: [
      "The agent header's chip now starts with the workspace title, not feat/empty-event-message",
      "The long title ends in an ellipsis inside the chip instead of overflowing",
    ],
  });

  // The chip sits right of the "Claude 1" title in the header.
  await page.mouse.move(470, 617);
  await settle(page, 1200);
  await capture(page, {
    name: "25-terminal-workspace-title-tooltip",
    title: "Hovering the chip shows the full title",
    screen: true,
    rest: false,
    expectations: ["A tooltip by the chip shows the full workspace title"],
  });
});
