// Renaming terminal columns: double-click a column's title to edit it in
// place. An agent keeps its new name in the session table; a shell keeps
// its name while it runs. Enter saves, Esc cancels.
import { expect, test } from "@playwright/test";
import { capture } from "./capture.ts";
import { openTab, openWorkspace, row, settle } from "./helpers.ts";

const BRANCH = "feat/keyvalues-cache";

test.describe.configure({ mode: "serial" });

test("terminal rename: agent and shell titles", async ({ page }) => {
  await openWorkspace(page, BRANCH);
  await openTab(page, "Commits");
  await row(page, BRANCH).hover();
  await row(page, BRANCH).getByRole("button", { name: "Start agent" }).click();
  await settle(page, 3000);
  await row(page, BRANCH).hover();
  await row(page, BRANCH).getByRole("button", { name: "Open shell" }).click();
  await settle(page, 2500);

  const agent = page.getByTestId(/^terminal-title-session-/).first();
  const shell = page.getByTestId(/^terminal-title-shell-/).first();
  await expect(agent).toBeVisible();
  await expect(shell).toBeVisible();

  await agent.dblclick();
  await settle(page, 600);
  await capture(page, {
    name: "24-terminal-rename-agent-editing",
    title: "Agent title being renamed",
    clip: { x: 0, y: 560, width: 1400, height: 110 },
    expectations: [
      "The agent column's title is replaced by a text field holding its name, selected",
      "The field sits where the title was; the workspace badge and buttons don't move",
    ],
  });
  await page.keyboard.type("Fix cache eviction");
  await page.keyboard.press("Enter");
  await settle(page, 800);
  await expect(page.getByTestId(/^terminal-title-session-/).first()).toHaveText("Fix cache eviction");

  await shell.dblclick();
  await settle(page, 600);
  await page.keyboard.type("dev server");
  await page.keyboard.press("Enter");
  await settle(page, 800);
  await expect(page.getByTestId(/^terminal-title-shell-/).first()).toHaveText("dev server");
  await capture(page, {
    name: "24-terminal-rename-renamed",
    title: "Agent and shell renamed",
    expectations: [
      "The agent column's header reads 'Fix cache eviction', the shell's 'dev server'",
      "Both still show their workspace badge and close button",
      "The sidebar lists the agent as 'Fix cache eviction'",
    ],
  });

  // Esc leaves the name as it was.
  await page.getByTestId(/^terminal-title-shell-/).first().dblclick();
  await settle(page, 400);
  await page.keyboard.type("typo");
  await page.keyboard.press("Escape");
  await settle(page, 600);
  await expect(page.getByTestId(/^terminal-title-shell-/).first()).toHaveText("dev server");
});
