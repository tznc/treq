// Selecting code text across diff lines, and commented lines shown as an amber
// band instead of a gutter icon. Runs on the alerting workspace's uncommitted
// edits (qa/launch.ts); positions are in the 1400x900 window, as in
// 24-diff-drag-selection.
import { expect, test, type Page } from "@playwright/test";
import { capture } from "./capture.ts";
import { openTab, openWorkspace, settle } from "./helpers.ts";

test.describe.configure({ mode: "serial" });

const GUTTER_X = 600;
const CODE_X = 760;
const FIRST = 245; // alerting.ts's first removed line
const ROW = 24;
const y = (row: number) => FIRST + row * ROW;

async function changes(page: Page): Promise<void> {
  await openWorkspace(page, "feat/alerting-logs");
  await openTab(page, "Changes");
  await settle(page, 1500);
}

async function drag(page: Page, x0: number, y0: number, x1: number, y1: number) {
  await page.mouse.move(x0, y0);
  await page.mouse.down();
  const steps = 8;
  for (let i = 1; i <= steps; i++) {
    await page.mouse.move(x0 + ((x1 - x0) * i) / steps, y0 + ((y1 - y0) * i) / steps);
    await settle(page, 60);
  }
  await page.mouse.up();
  await settle(page, 300);
}

test("diff text selection and commented lines", async ({ page }) => {
  await changes(page);
  await drag(page, CODE_X, y(0), CODE_X + 200, y(3));
  await capture(page, {
    name: "25-diff-01-text-selection",
    title: "Dragging over code selects text across lines",
    expectations: [
      "Code text from the first removed line to the fourth row is highlighted as a text selection",
      "The line-number gutter is not highlighted blue: no line range was started",
    ],
  });

  await page.keyboard.press("Escape");
  await drag(page, GUTTER_X, y(4), GUTTER_X, y(6));
  await page.keyboard.press("ArrowLeft");
  const input = page.getByRole("textbox", { name: "Add a comment…" });
  await expect(input).toBeVisible();
  await input.fill("Do these need a level too?");
  await page.getByRole("button", { name: "Add comment", exact: true }).click();
  await expect(input).toHaveCount(0);
  await page.mouse.move(1300, 850);
  await settle(page, 800);
  await capture(page, {
    name: "25-diff-02-commented-lines",
    title: "Commented lines carry an amber band, not an icon",
    expectations: [
      "The three commented rows have an amber background across the gutter and the code",
      "No chat-bubble icon appears in the line-number gutter",
      "The comment card sits under the commented rows, on the same amber as them",
    ],
  });
});
