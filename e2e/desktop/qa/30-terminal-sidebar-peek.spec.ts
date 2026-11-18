// Hovering a terminal column highlights, faintly, the sidebar row of the
// workspace the terminal belongs to.
import { expect, test } from "@playwright/test";
import { capture } from "./capture.ts";
import { openWorkspace, row, settle } from "./helpers.ts";

const OWNER = "feat/empty-event-message";
const OTHER = "feat/keyvalues-cache";

test.describe.configure({ mode: "serial" });

test("terminal sidebar peek: hovering a terminal highlights its workspace", async ({ page }) => {
  await openWorkspace(page, OWNER);
  await row(page, OWNER).hover();
  await row(page, OWNER).getByRole("button", { name: "Start agent" }).click();
  await settle(page, 3000);
  // Show another workspace: the agent's column stays, dimmed.
  await row(page, OTHER).click();
  await settle(page, 1500);

  const column = page.getByRole("group", { name: "Claude 1 terminal" });
  await expect(column).toBeVisible();
  await page.mouse.move(5, 5);
  await settle(page, 500);
  await capture(page, {
    name: "30-terminal-sidebar-peek-01-idle",
    title: "No terminal hovered",
    expectations: [`Only ${OTHER} is highlighted in the sidebar (selected)`],
  });

  await column.hover();
  await settle(page, 500);
  await capture(page, {
    name: "30-terminal-sidebar-peek-02-hovered",
    title: "Hovering the agent terminal",
    rest: false,
    expectations: [
      `The ${OWNER} sidebar row shows a faint hover highlight`,
      `${OTHER} stays the selected row`,
    ],
  });
});
