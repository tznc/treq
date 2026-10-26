// An agent column's workspace chip shows the workspace's title (synced
// from its PR by the fake `gh`), not its branch; a title too long for the
// chip ends in an ellipsis and hovering shows it whole.
import { expect, test } from "@playwright/test";
import { capture } from "./capture.ts";
import { openTab, openWorkspace, row, settle } from "./helpers.ts";

const BRANCH = "feat/empty-event-message";
const TITLE = "feat: handle empty event messages";

test.describe.configure({ mode: "serial" });

test("terminal workspace title: the agent chip names the workspace's title", async ({ page }) => {
  await openWorkspace(page, BRANCH);
  await openTab(page, "Commits");
  await row(page, BRANCH).hover();
  await row(page, BRANCH).getByRole("button", { name: "Start agent" }).click();
  await settle(page, 3000);

  const chip = page.getByTestId("terminal-workspace-label").first();
  await expect(chip).toHaveText(TITLE);
  await capture(page, {
    name: "25-terminal-workspace-title-chip",
    title: "Agent column header with the workspace title in its chip",
    clip: { x: 0, y: 560, width: 1400, height: 110 },
    expectations: [
      "The agent header's chip reads the workspace title, not feat/empty-event-message",
      "A title too long for the chip ends in an ellipsis instead of overflowing",
    ],
  });

  await chip.hover();
  await settle(page, 1200);
  await capture(page, {
    name: "25-terminal-workspace-title-tooltip",
    title: "Hovering the chip shows the full title",
    screen: true,
    rest: false,
    expectations: ["A tooltip above the chip shows the full workspace title"],
  });
});
