// The agent prompt dialog against the real backend, opened from the Code
// tab's Agent button and from the palette: Send starts the agent CLI (a fake
// `claude` that records its arguments).
import { expect, test } from "@playwright/test";
import { agentCalls, dialogPage, goHome, mod } from "../fixture.ts";

test("the home Code tab's Agent button starts a session", async ({
  page,
}) => {
  await goHome(page);
  await page.getByRole("button", { name: "Agent", exact: true }).click();
  const dialog = page.getByRole("dialog", {
    name: "Start a new agent session",
  });
  await expect(dialog).toBeVisible();

  const before = agentCalls().length;
  await dialog
    .getByRole("textbox", { name: "Describe a task…" })
    .fill("say hello from e2e");
  await dialog.getByRole("button", { name: "Send" }).click();
  await expect(dialog).toHaveCount(0);
  await expect
    .poll(() => agentCalls().slice(before).join("\n"), { timeout: 30_000 })
    .toContain("say hello from e2e");
});

test("the palette's prompt dialog starts a session", async ({
  page,
  context,
}) => {
  await goHome(page);
  await page.keyboard.press(`${mod}+k`);
  const palette = await dialogPage(context);
  await palette
    .getByRole("option", { name: "Start a new agent session with a prompt" })
    .click();

  const dialog = page.getByRole("dialog", {
    name: "Start a new agent session",
  });
  await expect(dialog).toBeVisible();
  await expect(dialog.getByRole("combobox", { name: "Agent" })).toHaveValue(
    "Claude",
  );
  const before = agentCalls().length;
  await dialog
    .getByRole("textbox", { name: "Describe a task…" })
    .fill("prompt dialog task");
  await dialog.getByRole("button", { name: "Send" }).click();
  await expect(dialog).toHaveCount(0);
  await expect
    .poll(() => agentCalls().slice(before).join("\n"), { timeout: 30_000 })
    .toContain("prompt dialog task");
});
