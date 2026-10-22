// The Changes tab's file list fades a file once it is marked Viewed in
// the diff, and brings it back when the mark is cleared.
//
// File rows and the Viewed toggle are not observable, so the toggle is
// clicked at a fixed point (the first file card's, for this seed and the
// 1400x900 window); each step then settles.
import { test } from "@playwright/test";
import { capture } from "./capture.ts";
import { openTab, openWorkspace, settle } from "./helpers.ts";

/** The first file card's Viewed toggle, for feat/empty-event-message. */
const VIEWED = { x: 1275, y: 188 };

test("changes viewed: a viewed file fades in the file list", async ({ page }) => {
  await openWorkspace(page, "feat/empty-event-message");
  await openTab(page, "Changes");
  await page.mouse.move(900, 600);
  await settle(page, 1500);
  await capture(page, {
    name: "16-changes-viewed-01-none",
    title: "Changes tab: no file marked viewed",
    expectations: ["Every row under Committed has full-strength name, folder and status"],
  });
  await page.mouse.click(VIEWED.x, VIEWED.y);
  await page.mouse.move(900, 600);
  await settle(page, 1000);
  await capture(page, {
    name: "16-changes-viewed-02-marked",
    title: "First file marked Viewed",
    expectations: [
      "The first card's Viewed toggle is checked and the card is collapsed",
      "That file's row in the list is greyed out (name, folder and status)",
      "The other rows keep their full-strength text",
    ],
  });
  await page.mouse.click(VIEWED.x, VIEWED.y);
  await page.mouse.move(900, 600);
  await settle(page, 1000);
  await capture(page, {
    name: "16-changes-viewed-03-unmarked",
    title: "Viewed mark cleared",
    expectations: ["The row is back to full-strength text, like the others"],
  });
});
