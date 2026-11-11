// Shift+wheel over the terminal row scrolls the columns sideways: down goes
// right, up goes left.
import { expect, test } from "@playwright/test";
import { capture } from "./capture.ts";
import { mod, openTab, openWorkspace, row, settle } from "./helpers.ts";

const BRANCH = "feat/keyvalues-cache";

test("terminal: shift+scroll moves the row of columns", async ({ page }) => {
  await openWorkspace(page, BRANCH);
  await openTab(page, "Commits");
  await row(page, BRANCH).hover();
  await row(page, BRANCH).getByRole("button", { name: "Open shell" }).click();
  await settle(page, 2500);
  for (let i = 0; i < 4; i++) {
    await page.keyboard.press(`${mod}+\\`);
    await settle(page, 1500);
  }
  const first = page.getByRole("group", { name: "Shell terminal" }).first();
  const x = async () => (await first.boundingBox())!.x;
  const box = (await page.getByRole("group", { name: "Shell terminal" }).last().boundingBox())!;
  const shiftWheel = async (dy: number) => {
    // `capture` parks the pointer, so hover the row again.
    await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
    await page.keyboard.down("Shift");
    await page.mouse.wheel(0, dy);
    await page.keyboard.up("Shift");
  };

  // Start from the left end.
  await shiftWheel(-100_000);
  await settle(page, 600);
  const start = await x();
  await capture(page, {
    name: "29-terminal-shift-scroll-01-left",
    title: "Terminal row at its left end",
    expectations: ["The first Shell column starts at the row's left edge"],
  });

  await shiftWheel(400); // down
  await settle(page, 600);
  const afterDown = await x();
  expect(afterDown).toBeLessThan(start);
  await capture(page, {
    name: "29-terminal-shift-scroll-02-down-right",
    title: "After shift+scroll down",
    expectations: [
      "The row scrolled right: the first column moved left, partly out of view",
      "The scrollbar under the row moved right",
    ],
  });

  await shiftWheel(-400); // up
  await settle(page, 600);
  expect(await x()).toBeGreaterThan(afterDown);
  await capture(page, {
    name: "29-terminal-shift-scroll-03-up-left",
    title: "After shift+scroll up",
    expectations: ["The row scrolled back left to where it started"],
  });
});
