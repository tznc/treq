import * as React from "react";
import { it } from "vitest";
import userEvent from "@testing-library/user-event";
import { createTestRepo, openRepo } from "../../../test/utils";
import { render, screen } from "../../../test/test-utils";
import { Dashboard } from "../../../src/components/Dashboard";
import { saveAppSetupScript } from "../../../src/lib/api";
import { captureDocument } from "../capture";

it("captures the application setup script settings", async () => {
  const { repoPath } = createTestRepo(false);
  openRepo(repoPath);
  await saveAppSetupScript("", false);

  const user = userEvent.setup();
  render(<Dashboard />);
  await user.click(await screen.findByLabelText("Settings"));
  await user.click(await screen.findByRole("tab", { name: /application/i }));
  const textarea = await screen.findByLabelText("Application Setup Script");
  await screen.findByText("Never run");
  await captureDocument(document, {
    name: "app-setup-script-settings-01-empty",
    viewport: { width: 1440, height: 1200 },
    expectations: [
      "An 'Application Setup Script' textarea with a monospace placeholder sits in the Application tab.",
      "A 'Run on every app startup' switch is shown in the off state with its label beside it.",
      "The status line under the switch reads 'Never run'.",
    ],
  });

  await user.type(textarea, "echo setup");
  await user.click(
    screen.getByRole("switch", { name: "Run on every app startup" }),
  );
  await user.click(screen.getByRole("button", { name: /save settings/i }));
  await screen.findByText(/Last run .* passed/);
  await captureDocument(document, {
    name: "app-setup-script-settings-02-ran",
    viewport: { width: 1440, height: 1200 },
    expectations: [
      "The textarea shows 'echo setup' in monospace.",
      "The 'Run on every app startup' switch is on (filled primary color).",
      "The status line reads 'Last run <timestamp> · passed'.",
    ],
  });
}, 60000);
