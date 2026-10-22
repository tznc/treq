// A workspace whose checkout was deleted behind the app: the Code tab's
// commit read fails, and the Commits panel says so instead of showing
// "Loading commits..." forever. The checkout is moved back afterwards so
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
        "The Commits panel says 'Couldn't load commits' with the read's error under it",
        "No 'Loading commits...' text is left on screen",
      ],
    });
    await expect(page.getByRole("status", { name: "Couldn't load commits" })).toBeVisible();
  } finally {
    renameSync(`${dir}.gone`, dir);
  }
});
