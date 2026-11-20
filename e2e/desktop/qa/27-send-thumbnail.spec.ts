// `treq send` file thumbnails are square chips with centered names.
import { test } from "@playwright/test";
import { appBinary } from "../env.ts";
import { capture } from "./capture.ts";
import { openTab, openWorkspace, row, settle } from "./helpers.ts";

const BRANCH = "feat/keyvalues-cache";

test("send thumbnail: square chip, centered name", async ({ page }) => {
  await openWorkspace(page, BRANCH);
  await openTab(page, "Commits");
  await row(page, BRANCH).hover();
  await row(page, BRANCH).getByRole("button", { name: "Open shell" }).click();
  await settle(page, 2500);
  await page.mouse.click(1000, 780);
  await page.keyboard.type("PS1='$ '; echo hi > privacy.md; echo hi > a.txt; echo hi > treq-force-rebase-handoff.md\n");
  await page.keyboard.type(
    `for f in privacy.md a.txt treq-force-rebase-handoff.md; do '${appBinary}' send $f; done; clear\n`,
  );
  await settle(page, 1500);
  await capture(page, {
    name: "27-send-thumbnail-chips",
    title: "Attachment chips: square, centered names",
    expectations: [
      "Each file chip is square (as wide as it is tall)",
      "privacy.md and a.txt are centered under their icons",
      "The long name shows its start and ends in an ellipsis inside the chip",
    ],
  });
});
