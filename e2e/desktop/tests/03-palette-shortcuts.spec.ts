// command-palette.test.tsx and keyboard-shortcuts-modal.test.tsx against
// the real app: the shortcuts open their dialogs (which GPUI draws in a
// window over the repo window) and their commands act on the window.
import { expect, test } from "@playwright/test";
import { dialogPage, goHome, mod } from "../fixture.ts";

test("Cmd+K opens the palette with Go to Home and Go to Settings", async ({
  page,
  context,
}) => {
  await goHome(page);
  await page.keyboard.press(`${mod}+k`);
  const palette = await dialogPage(context);
  await expect(
    palette.getByRole("option", { name: "Go to home" }),
  ).toBeVisible();
  await expect(
    palette.getByRole("option", { name: "Go to settings" }),
  ).toBeVisible();
  await expect(
    palette.getByRole("option", { name: "Search files" }),
  ).toBeVisible();
  await expect(
    palette.getByTitle("Jump to a file in the repository"),
  ).toBeVisible();

  await palette.getByRole("option", { name: "Go to settings" }).click();
  await expect(
    page.getByRole("button", { name: "Save settings" }),
  ).toBeVisible();
  await page.getByRole("button", { name: "Close" }).click();
});

test("the palette filters its commands as you type", async ({
  page,
  context,
}) => {
  await goHome(page);
  await page.getByRole("button", { name: "Command palette" }).click();
  const palette = await dialogPage(context);
  await palette
    .getByRole("textbox", { name: "Type a command or search…" })
    .fill("branch");
  await expect(
    palette.getByRole("option", { name: "Switch branch" }),
  ).toBeVisible();
  await expect(palette.getByRole("option", { name: "Go to home" })).toHaveCount(
    0,
  );
  await palette.keyboard.press("Escape");
});

test("Search Files opens the file picker", async ({ page, context }) => {
  await goHome(page);
  await page.keyboard.press(`${mod}+k`);
  const palette = await dialogPage(context);
  await palette.getByRole("option", { name: "Search files" }).click();
  const search = page.getByRole("textbox", { name: "Search files…" });
  await expect(search).toBeVisible();
  await search.fill("READ");
  await expect(page.getByRole("option", { name: "README.md" })).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(search).toHaveCount(0);
});

test("? opens the keyboard shortcuts, but not while typing", async ({
  page,
  context,
}) => {
  await goHome(page);
  await page.getByRole("button", { name: "Agent", exact: true }).click();
  const prompt = page.getByRole("dialog", { name: "Start a new agent session" });
  const task = prompt.getByRole("textbox", { name: "Describe a task…" });
  await task.click();
  await task.pressSequentially("why?");
  await expect(task).toHaveValue("why?");
  await task.fill("");
  await page.keyboard.press("Escape");
  await expect(prompt).toHaveCount(0);

  await page.getByRole("tab", { name: "Commits" }).click();
  await page.keyboard.press("Shift+?");
  const dialog = await dialogPage(context);
  await expect(
    dialog.getByRole("dialog", { name: "Keyboard shortcuts" }),
  ).toBeVisible();
  await dialog.keyboard.press("Escape");
  await page.getByRole("tab", { name: "Code" }).click();
});
