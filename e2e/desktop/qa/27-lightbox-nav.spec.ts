// The lightbox's previous/next buttons float at the window's edges when it
// is wide, and shrink into the actions row when it is narrow, clear of the
// attachment.
import { expect, test } from "@playwright/test";
import { appBinary } from "../env.ts";
import { capture } from "./capture.ts";
import { openTab, openWorkspace, row, settle } from "./helpers.ts";

const BRANCH = "feat/keyvalues-cache";

test("lightbox nav: edges when wide, actions row when narrow", async ({ page }) => {
  await openWorkspace(page, BRANCH);
  await openTab(page, "Commits");
  await row(page, BRANCH).hover();
  await row(page, BRANCH).getByRole("button", { name: "Open shell" }).click();
  await settle(page, 2500);
  await page.mouse.click(1000, 780);
  await page.keyboard.type("PS1='$ '; seq 1 80 | sed 's/^/A long line of attachment text, number /' > a.txt; echo b > b.txt\n");
  await page.keyboard.type(`for f in a.txt b.txt; do '${appBinary}' send $f; done; clear\n`);
  await settle(page, 1500);
  // The cards draw their names, which Playwright cannot query.
  await page.mouse.click(364, 693);
  await settle(page, 1000);
  const next = page.getByRole("button", { name: "Next" });
  const close = page.getByRole("button", { name: "Close" }).last();
  await expect(next).toBeVisible();
  await capture(page, {
    name: "27-lightbox-nav-01-wide",
    title: "Wide window: large arrows at the edges",
    expectations: [
      "Large round previous/next arrows float at the left and right edges, vertically centered",
      "The actions row (copy, reveal, close, review input) holds no arrows",
    ],
  });
  await page.setViewportSize({ width: 700, height: 900 });
  await settle(page, 800);
  const [n, c] = [await next.boundingBox(), await close.boundingBox()];
  expect(n!.width).toBeLessThanOrEqual(40);
  expect(Math.abs(n!.y + n!.height / 2 - (c!.y + c!.height / 2))).toBeLessThan(12);
  await capture(page, {
    name: "27-lightbox-nav-02-narrow",
    title: "Narrow window: small arrows in the actions row",
    expectations: [
      "Small previous/next arrows sit in the actions row, on either side of the toolbar and review input",
      "No arrow overlaps the attachment text",
    ],
  });
});
