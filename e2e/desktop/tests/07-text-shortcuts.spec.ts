// The standard text-editing shortcuts in the app's text fields, pressed as
// real key events through the window system (xdotool on X11), so a menu
// accelerator or the platform layer swallowing them fails here. Playwright's
// own keyboard injects into GPUI and would skip that layer.
import { execFileSync } from "node:child_process";
import { expect, test, type Locator } from "@playwright/test";
import { dialogPage, goHome } from "../fixture.ts";

test.skip(process.platform !== "linux", "presses keys with xdotool");

/** Presses `keys` (xdotool's names, e.g. "ctrl+v") in the focused window. */
function press(keys: string) {
  execFileSync("xdotool", ["key", "--clearmodifiers", keys]);
}

/** Puts `text` on the clipboard through the app, as copying it would. */
async function clipboard(field: Locator, text: string) {
  await field.fill(text);
  press("ctrl+a");
  press("ctrl+c");
}

async function editsWithShortcuts(field: Locator) {
  await field.click();
  await clipboard(field, "pasted");
  press("ctrl+a");
  press("BackSpace");
  await expect(field).toHaveValue("");

  press("ctrl+v");
  await expect(field).toHaveValue("pasted");
  press("ctrl+v");
  await expect(field).toHaveValue("pastedpasted");

  press("ctrl+z");
  await expect(field).toHaveValue("pasted");

  press("ctrl+a");
  press("ctrl+x");
  await expect(field).toHaveValue("");
  press("ctrl+v");
  await expect(field).toHaveValue("pasted");
  await field.fill("");
}

test("the home task field takes paste, select all, cut and undo", async ({
  page,
}) => {
  await goHome(page);
  await editsWithShortcuts(
    page.getByRole("textbox", { name: "Describe a task…" }),
  );
});

test("the file search takes paste, select all, cut and undo", async ({
  page,
  context,
}) => {
  await goHome(page);
  await page.getByRole("button", { name: "Command palette" }).click();
  const palette = await dialogPage(context);
  await palette.getByRole("option", { name: "Search files" }).click();
  // The picker draws in the repo window, which keeps X's input focus;
  // xdotool can't move focus to the palette's own window without a window
  // manager.
  await editsWithShortcuts(
    page.getByRole("textbox", { name: "Search files…" }),
  );
  await page.keyboard.press("Escape");
});

test("the New workspace lines take paste, select all, cut and undo", async ({
  page,
}) => {
  await goHome(page);
  await page.getByRole("button", { name: "New workspace…" }).first().click();
  await expect(
    page.getByRole("heading", { name: "New workspace" }),
  ).toBeVisible();
  await editsWithShortcuts(page.getByRole("textbox", { name: "Workspaces" }));
  await page.keyboard.press("Escape");
});
