// OpenCode, an agent CLI the QA fixture installs beside Claude: the agent
// prompt's picker lists it, and Settings › Application offers it as the
// default agent and lists it among the command-line tools.
//
// A select's options are not observable: the open list is captured as is.
import { expect, test, type Page } from "@playwright/test";
import { capture } from "./capture.ts";
import { mod, openWorkspace, ready, settle } from "./helpers.ts";

test.describe.configure({ mode: "serial" });

async function application(page: Page): Promise<void> {
  await ready(page);
  await page.getByRole("button", { name: "Settings", exact: true }).click();
  await page.getByRole("tab", { name: "Application" }).click();
  await expect(page.getByRole("tab", { name: "Application", selected: true })).toBeVisible();
  await page.mouse.move(1000, 860);
  await settle(page, 1200);
}

test("opencode: the agent prompt's picker lists OpenCode", async ({ page }) => {
  await openWorkspace(page, "feat/keyvalues-cache");
  await page.keyboard.press(`${mod}+i`);
  await expect(page.getByRole("dialog", { name: "Start a new agent session" })).toBeVisible();
  await settle(page, 1000);
  await page.getByRole("combobox", { name: "Agent" }).click();
  await settle(page, 900);
  await capture(page, {
    name: "33-opencode-agent-picker",
    title: "Agent prompt: agent picker with OpenCode installed",
    screen: true,
    rest: false,
    expectations: [
      "Claude and OpenCode are listed, each with its icon; Claude is checked",
      "Agents whose CLI isn't installed are not listed",
    ],
  });
  await page.keyboard.press("Escape");
  await settle(page, 500);
  await page.keyboard.press("Escape");
  await settle(page, 500);
});

test("opencode: Settings › Application offers OpenCode", async ({ page }) => {
  await application(page);
  const select = page.getByRole("combobox", { name: "Default agent" });
  await select.click();
  await settle(page, 800);
  await capture(page, {
    name: "33-opencode-default-agent",
    title: "Settings: Default agent select open",
    expectations: ["Default (Claude), Claude, Codex, Cursor, Copilot, Devin, Gemini and OpenCode are listed"],
  });
  await page.keyboard.press("Escape");
  await settle(page, 500);
  await page.getByText("Command-line tools", { exact: true }).scrollIntoViewIfNeeded();
  await settle(page, 800);
  await capture(page, {
    name: "33-opencode-command-line-tools",
    title: "Settings: Command-line tools",
    expectations: [
      "OpenCode is listed under the agent CLIs after Gemini CLI",
      "OpenCode is marked installed; uninstalled agents show an Install link",
    ],
  });
});
