// A workspace whose checkout was deleted behind the app: the Code tab's
// commit read fails, and the Commits panel says so instead of showing
// "Loading commits…" forever, and "Show error" opens the error in a
// popover to copy from. The files panel says the folder can't be read,
// with its own "Show error" and no Retry. The checkout is moved back afterwards so
// later specs see the seed unchanged.
import { renameSync } from "node:fs";
import { expect, test } from "@playwright/test";
import { fixture } from "../fixture.ts";
import { CACHE, workspaceDir } from "../landing/seed.ts";
import { capture } from "./capture.ts";
import { openTab, openWorkspace, settle } from "./helpers.ts";

test.describe.configure({ mode: "serial" });

test("commits-error: a failed commit read is reported, not left loading", async ({ page }) => {
  const dir = workspaceDir(fixture().repo, CACHE);
  renameSync(dir, `${dir}.gone`);
  try {
    await openWorkspace(page, CACHE);
    await openTab(page, "Code");
    await settle(page, 3000);
    await capture(page, {
      name: "15-commits-error",
      title: "Code tab of a workspace whose checkout was deleted",
      expectations: [
        "The Commits panel says 'Couldn't load commits' with only a 'Show error' button: no Retry, no message line",
        "The files panel says 'Can't show this workspace's files' with one sentence and 'Show error': no Retry, no raw path, no README box",
        "No 'Loading commits…' text is left on screen",
      ],
    });
    await expect(page.getByRole("status", { name: "Couldn't load commits" })).toBeVisible();
    await expect(page.getByRole("status", { name: "Can't show this workspace's files" })).toBeVisible();
    await expect(page.getByRole("button", { name: "Retry" })).toHaveCount(0);

    await page.getByTestId("code-history-show-error").click();
    await settle(page, 800);
    await capture(page, {
      name: "15-commits-error-popover",
      title: "The commit read error in its popover",
      expectations: [
        "A popover under 'Show error' holds the full error in a padded, wrapping monospace text area",
        "The popover stays inside the window",
      ],
    });
    await expect(page.getByRole("textbox", { name: "Commit read error" })).toBeVisible();
    await page.keyboard.press("Escape");

    await page.getByTestId("code-files-show-error").click();
    await settle(page, 800);
    await expect(page.getByRole("textbox", { name: "File read error" })).toBeVisible();
  } finally {
    renameSync(`${dir}.gone`, dir);
  }
});
