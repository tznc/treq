// An agent header's workspace label: the name goes to that workspace, the
// `+N -N` counts go to its Changes tab.
import { expect, test } from "@playwright/test";
import { capture } from "./capture.ts";
import { openTab, openWorkspace, row, settle } from "./helpers.ts";

const BRANCH = "feat/keyvalues-cache";
const OTHER = "feat/empty-event-message";

test.describe.configure({ mode: "serial" });

const selectedRow = (page, branch: string) =>
  page.getByRole("button", { name: branch, exact: true, selected: true });

test("terminal label: name and diff counts navigate", async ({ page }) => {
  await openWorkspace(page, OTHER);
  await row(page, OTHER).hover();
  await row(page, OTHER).getByRole("button", { name: "Start agent" }).click();
  await settle(page, 3000);
  await openWorkspace(page, BRANCH);
  await openTab(page, "Commits");
  await capture(page, {
    name: "21-terminal-label-01-before",
    title: "On BRANCH's Commits, with OTHER's agent in the pane",
    expectations: ["The agent header shows OTHER's workspace label with its +N -N counts"],
  });

  const label = page.getByTestId("terminal-workspace-label").first();
  await label.click({ position: { x: 8, y: 6 } });
  await expect(selectedRow(page, OTHER)).toBeVisible();
  await expect(page.getByRole("tab", { name: "Commits", selected: true })).toBeVisible();
  await settle(page);
  await capture(page, {
    name: "21-terminal-label-02-name-click",
    title: "After clicking the label name",
    expectations: ["OTHER is selected in the sidebar", "The Commits tab stays selected"],
  });

  await openWorkspace(page, BRANCH);
  await openTab(page, "Commits");
  await page.getByRole("button", { name: "Open Changes" }).first().click();
  await expect(selectedRow(page, OTHER)).toBeVisible();
  await expect(page.getByRole("tab", { name: "Changes", selected: true })).toBeVisible();
  await settle(page);
  await capture(page, {
    name: "21-terminal-label-03-counts-click",
    title: "After clicking the +N -N counts",
    expectations: ["OTHER is selected in the sidebar", "The Changes tab is selected"],
  });
});
