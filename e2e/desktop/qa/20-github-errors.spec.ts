// The GitHub view's error state when gh is signed out: a distinct
// "not authenticated" message, Try again, and Copy error.
import { rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { expect, test } from "@playwright/test";
import { fixture } from "../fixture.ts";
import { capture } from "./capture.ts";
import { ready, settle } from "./helpers.ts";

const signedOut = () => join(fixture().home, "fake-gh-signed-out");

test.afterAll(() => rmSync(signedOut(), { force: true }));

test("github-errors: signed-out gh", async ({ page }) => {
  writeFileSync(signedOut(), "");
  await ready(page);
  await page.getByRole("button", { name: "Github", exact: true }).click();
  await settle(page, 2500);
  await capture(page, {
    name: "20-github-issues-unauthenticated",
    title: "GitHub view: gh signed out",
    screen: true,
    expectations: [
      "Red alert icon, 'Could not load issues', then 'gh is not authenticated. Run `gh auth login`…'",
      "'Try again' and 'Copy error' buttons side by side, centered",
    ],
  });
  await expect(page.getByRole("button", { name: "Copy error" })).toBeVisible();
  await page.getByRole("button", { name: "Copy error" }).click();
});
