// TREQ-45: an alert in the Code tab's alerts popover shows the workspace
// setup script's state; View logs opens the Logs tab on the setup script's run.
import { rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { expect, test } from "@playwright/test";
import { fixture } from "../fixture.ts";
import { capture } from "./capture.ts";
import { mod, openTab, openWorkspace, overlay, settle } from "./helpers.ts";

const BRANCH = "feat/empty-event-message";

test("setup-output: Code tab setup alert", async ({ page, context }) => {
  const config = join(fixture().repo, ".treq", "config.yaml");
  writeFileSync(
    config,
    `setup_script: ${JSON.stringify(
      "echo installing deps\necho 'error: lockfile out of date' 1>&2\nexit 1",
    )}\n`,
  );
  try {
    // The workspace predates the setup script, so it reads as pending (which
    // disables the alert's re-run button); the palette runs it regardless.
    await openWorkspace(page, BRANCH);
    await page.keyboard.press(`${mod}+k`);
    const palette = await overlay(page, context);
    await palette.getByRole("textbox", { name: "Type a command or search…" }).fill("setup");
    await palette.getByRole("option", { name: "Re-run setup script" }).click();
    await settle(page, 3000);
    await page.keyboard.press("Escape");
    await openTab(page, "Code");
    const alerts = page.getByRole("button", { name: /^Alerts/ });
    await expect(alerts).toBeVisible({ timeout: 30_000 });
    // The alert reads failed once the run ends; reopen the popover until then.
    const viewLogs = page.getByRole("button", { name: "View logs" });
    await expect(async () => {
      if ((await viewLogs.count()) === 0) {
        await page.keyboard.press("Escape");
        await alerts.click();
      }
      await expect(viewLogs).toBeVisible({ timeout: 2_000 });
    }).toPass({ timeout: 30_000 });
    await expect(viewLogs).toBeVisible({ timeout: 30_000 });
    await settle(page, 1000);
    await capture(page, {
      name: "25-setup-output-01-alert",
      title: "Code tab: failed setup script alert",
      expectations: [
        "The alerts popover under the alerts button holds a red alert reading 'Workspace setup script failed' / 'Checks can still run.'",
        "Its actions are 'View logs' and 'Re-run setup script'; no script output shows on the Code tab",
      ],
    });

    await viewLogs.click();
    await expect(page.getByRole("tab", { name: /^Logs/, selected: true })).toBeVisible();
    // Log lines are drawn text, not observable elements: let them load.
    await settle(page, 3000);
    await capture(page, {
      name: "25-setup-output-02-logs",
      title: "Logs tab: the setup script's run",
      expectations: [
        "The Logs tab is selected, and its check filter reads 'Setup script'",
        "The log lines are the setup script's: 'installing deps' and the stderr 'error: lockfile out of date'",
      ],
    });
  } finally {
    rmSync(config, { force: true });
  }
});
