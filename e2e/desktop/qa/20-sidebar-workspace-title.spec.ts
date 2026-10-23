// A titled workspace's sidebar row: the title in the sans face, truncated
// before the row's status, with the full title and the mono branch in its
// tooltip. Untitled rows keep their mono branch. The test creates the
// titled workspace through the New Workspaces dialog and deletes it again,
// so the sidebar is back to the seed for later areas.
import { expect, test, type BrowserContext, type Locator, type Page } from "@playwright/test";
import { capture } from "./capture.ts";
import { home, mod, overlay, ready, row, settle } from "./helpers.ts";

test.describe.configure({ mode: "serial" });

const TITLE = "Fail readiness when the spool queue is unreachable for longer than a minute";

/** The page a dialog renders in: the repo window, else the shared dialog window. */
async function dialogIn(
  page: Page,
  context: BrowserContext,
  find: (p: Page) => Locator,
): Promise<Page> {
  try {
    await expect(find(page)).toBeVisible({ timeout: 4000 });
    return page;
  } catch {
    const other = await overlay(page, context);
    await expect(find(other)).toBeVisible();
    return other;
  }
}

const heading = (p: Page) => p.getByRole("heading", { name: "New Workspaces" });

test("sidebar-workspace-title: a titled row shows its title", async ({ page, context }) => {
  await home(page);
  await page.keyboard.press(`${mod}+n`);
  const d = await dialogIn(page, context, heading);
  await d.getByLabel("Workspaces", { exact: true }).fill(TITLE);
  await settle(page, 1200);
  await d.getByRole("button", { name: "Create 1 Workspace" }).click();
  await expect(heading(d)).toHaveCount(0, { timeout: 30_000 });
  await ready(page);
  await settle(page, 1500);

  const titled = row(page, TITLE);
  await expect(titled).toBeVisible();
  const box = await titled.boundingBox();
  if (!box) throw new Error("row not drawn");
  await capture(page, {
    name: "20-sidebar-titled-row",
    title: "Titled workspace row among untitled ones",
    clip: { x: 0, y: Math.max(0, box.y - 120), width: box.x + box.width + 8, height: 240 },
    expectations: [
      "The titled row reads 'Fail readiness when…' in the sans face, ending in an ellipsis",
      "The other rows still show their branch names in mono",
      "The titled row is the same height as its neighbours",
    ],
  });

  await page.mouse.move(box.x + 60, box.y + box.height / 2);
  await settle(page, 1500);
  await capture(page, {
    name: "20-sidebar-titled-row-tooltip",
    title: "Tooltip of a titled workspace row",
    screen: true,
    rest: false,
    expectations: [
      "The tooltip shows the full title, wrapped if long, above the branch",
      "The branch line is mono with the branch icon",
    ],
  });

  // Back to the seed.
  await home(page);
  await page.keyboard.press(`${mod}+k`);
  const palette = await overlay(page, context);
  await palette.getByRole("textbox", { name: "Type a command or search..." }).fill("Delete Workspace");
  await palette.getByRole("option", { name: "Delete Workspace" }).click();
  const search = (p: Page) => p.getByRole("textbox", { name: "Search workspaces to delete..." });
  const picker = await dialogIn(page, context, search);
  await search(picker).fill("fail-readiness");
  await settle(page, 800);
  await picker.keyboard.press("Enter").catch(() => {});
  await page.getByTestId("confirm-confirm").click();
  await page.waitForTimeout(2500);
  await ready(page);
  await expect(row(page, TITLE)).toHaveCount(0);
});
