// Right-clicking a button that opens GitHub offers "Copy link", which copies
// its URL and toasts. feat/keyvalues-cache has open PR #401.
import { execFileSync } from "node:child_process";
import { expect, test } from "@playwright/test";
import { capture } from "./capture.ts";
import { focusMain, openWorkspace, popupBox, ready, settle, xPointer } from "./helpers.ts";

test.describe.configure({ mode: "serial" });

for (const [name, what, suffix] of [
  ["Open PR on web", "web", ""],
  [/^View PR /, "status", ""],
  [/checks/i, "checks", "/checks"],
] as const) {
  test(`copy-link: ${what} button`, async ({ page }) => {
    await ready(page);
    await openWorkspace(page, "feat/keyvalues-cache");
    await settle(page, 1500);
    await page.getByRole("button", { name }).first().click({ button: "right" });
    await settle(page, 900);
    focusMain();
    const menu = popupBox();
    if (!menu) throw new Error("no context menu");
    await capture(page, {
      name: `16-copy-link-${what}-menu`,
      title: `Right-click on the ${what} button`,
      screen: true,
      rest: false,
      expectations: [
        "A context menu opens at the pointer with one row, 'Copy link', and a link icon",
      ],
    });
    await xPointer(page, menu.x + menu.w / 2, menu.y + menu.h / 2, 1);
    await settle(page, 600);
    await capture(page, {
      name: `16-copy-link-${what}-toast`,
      title: `After Copy link on the ${what} button`,
      screen: true,
      rest: false,
      expectations: ["The menu is gone and a one-line 'Link copied' toast shows"],
    });
    const copied = execFileSync("xclip", ["-o", "-selection", "clipboard"], {
      encoding: "utf8",
    });
    expect(copied).toMatch(new RegExp(`/pull/401${suffix}$`));
  });
}
