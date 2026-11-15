// The project menu's New Window… opens a window on the start screen, not
// a blank one.
import { expect, test } from "@playwright/test";
import { windowPage } from "../fixture.ts";
import { capture } from "./capture.ts";
import { popupBox, ready, settle, xPointer } from "./helpers.ts";

test("new window: the project menu opens a window on the start screen", async ({
  page,
  context,
}) => {
  await ready(page);
  await page.getByRole("button", { name: /^Project: / }).click();
  await settle(page, 1000);
  const menu = popupBox();
  if (!menu) throw new Error("no project menu");
  // New Window… is the last row.
  await xPointer(page, menu.x + 60, menu.y + menu.h - 18, 1);
  const second = await windowPage(context);
  await second.setViewportSize({ width: 1400, height: 900 });
  await expect(second.getByRole("button", { name: "Open repository…" })).toBeVisible({
    timeout: 30_000,
  });
  await settle(second, 1500);
  await capture(second, {
    name: "21-new-window-01-start-screen",
    title: "New Window… from the project menu",
    screen: true,
    expectations: ["The new window shows the start screen: machines and recent repositories"],
  });
});
