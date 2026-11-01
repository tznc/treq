// home-task-new-workspace, agent-session-default-model and the agent
// prompt dialog against the real backend: a task typed on the home view
// starts the agent CLI (a fake `claude` that records its arguments).
import { expect, test } from "@playwright/test";
import { agentCalls, dialogPage, goHome, mod } from "../fixture.ts";

test("a home task starts the agent at home when the new-workspace switch is off", async ({
  page,
}) => {
  await goHome(page);
  const toggle = page.getByRole("switch", { name: "Run in a new workspace" });
  if (await toggle.isChecked()) await toggle.click();
  await expect(toggle).not.toBeChecked();

  const before = agentCalls().length;
  await page
    .getByRole("textbox", { name: "Describe a task…" })
    .fill("say hello from e2e");
  await page.getByRole("button", { name: "Edit", exact: true }).click();
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
  await dialog.getByRole("button", { name: "Edit", exact: true }).click();
  await expect(dialog).toHaveCount(0);
  await expect
    .poll(() => agentCalls().slice(before).join("\n"), { timeout: 30_000 })
    .toContain("prompt dialog task");
});
