// Dragging in the diff gutter selects lines as one blue band, across context
// and added lines, in both directions; the row under the cursor stays part of
// the band. Runs on the alerting workspace's uncommitted edits (qa/launch.ts).
import { test, type Page } from "@playwright/test";
import { capture } from "./capture.ts";
import { openTab, openWorkspace, settle } from "./helpers.ts";

test.describe.configure({ mode: "serial" });

// By place in the 1400x900 window, as in 13-changes-review: the gutter's x and
// alerting.ts's first removed line's y. The drag spans removed, added and the
// trailing context line.
const X = Number(process.env.QA_GUTTER_X ?? 600);
const TOP = Number(process.env.QA_GUTTER_Y ?? 245);
const ROWS = 8;
const ROW = 24;

async function changes(page: Page): Promise<void> {
  await openWorkspace(page, "feat/alerting-logs");
  await openTab(page, "Changes");
  await settle(page, 1500);
}

async function drag(page: Page, from: number, to: number, name: string, title: string) {
  await page.mouse.move(X, from);
  await page.mouse.down();
  const steps = Math.abs(to - from) / ROW;
  for (let i = 1; i <= steps; i++) {
    await page.mouse.move(X, from + Math.sign(to - from) * i * ROW);
    await settle(page, 60);
  }
  await settle(page, 300);
  await capture(page, {
    name: `${name}-held`,
    title: `${title}, mouse held`,
    expectations: [
      "The dragged rows form one continuous blue band across the line-number gutter",
      "The row under the cursor is blue in its gutter too, not grey",
      "Removed, added and context rows in the band share the same blue",
    ],
  });
  await page.mouse.up();
  await settle(page, 300);
  await capture(page, {
    name: `${name}-released`,
    title: `${title}, released`,
    expectations: ["The band is unchanged after release; the hovered row's gutter is still blue"],
  });
}

test("diff drag selection: one band in both directions", async ({ page }) => {
  await changes(page);
  await capture(page, {
    name: "24-drag-00-start",
    title: "Diff before any selection",
    expectations: ["Context lines and green added lines are visible in alerting.ts"],
  });
  await drag(page, TOP, TOP + ROWS * ROW, "24-drag-01-down", "Drag down from removed through added to context");
  await page.keyboard.press("Escape");
  await drag(page, TOP + ROWS * ROW, TOP, "24-drag-02-up", "Drag up from context through added to removed");
});
