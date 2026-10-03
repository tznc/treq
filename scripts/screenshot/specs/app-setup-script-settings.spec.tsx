import * as React from "react";
import { expect, it } from "vitest";
import userEvent from "@testing-library/user-event";
import { createTestRepo, openRepo } from "../../../test/utils";
import { render, screen, waitFor, within } from "../../../test/test-utils";
import { Dashboard } from "../../../src/components/Dashboard";
import { saveAppSetupScript } from "../../../src/lib/api";
import { captureDocument } from "../capture";

const SCRIPT = [
  "echo installing tools",
  "echo 'warning: jj already installed'",
  "echo 'error: brew not found' 1>&2",
  "echo setup done",
].join("\n");

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
  const run = screen.getByRole("button", { name: /^run$/i });
  expect(run).toBeDisabled();
  await captureDocument(document, {
    name: "app-setup-script-settings-01-empty",
    viewport: { width: 1440, height: 1200 },
    expectations: [
      "An 'Application Setup Script' textarea with a monospace placeholder sits in the Application tab.",
      "A 'Run on every app startup' switch is shown in the off state with its label beside it.",
      "A disabled 'Run' button sits below the switch, with 'Never run' beside it.",
    ],
  });

  await user.type(textarea, SCRIPT);
  await screen.findByText("Save settings to run the edited script.");
  expect(run).toBeDisabled();
  await captureDocument(document, {
    name: "app-setup-script-settings-02-unsaved",
    viewport: { width: 1440, height: 1200 },
    expectations: [
      "The textarea shows the four-line script in monospace.",
      "The 'Run' button is still disabled (greyed out).",
      "Beside it the hint reads 'Save settings to run the edited script.'",
    ],
  });

  await user.click(screen.getByRole("button", { name: /save settings/i }));
  await waitFor(() => expect(run).toBeEnabled());
  expect(screen.getByText("Never run")).toBeTruthy();
  await captureDocument(document, {
    name: "app-setup-script-settings-03-saved",
    viewport: { width: 1440, height: 1200 },
    expectations: [
      "The 'Run' button is enabled after saving.",
      "The status beside it still reads 'Never run': saving did not run the script.",
    ],
  });

  await user.click(run);
  await screen.findByText(/Last run .* passed/);
  await captureDocument(document, {
    name: "app-setup-script-settings-04-ran",
    viewport: { width: 1440, height: 1200 },
    expectations: [
      "Beside the enabled 'Run' button the status reads 'Last run <timestamp> · passed'.",
    ],
  });

  await user.click(screen.getByRole("button", { name: "Close" }));
  await user.click(await screen.findByRole("tab", { name: /^Logs/ }));
  await user.selectOptions(
    await screen.findByLabelText("Log source"),
    "app-setup",
  );
  const output = await screen.findByTestId("app-setup-logs-output");
  await within(output).findByText("setup done");
  await within(output).findByText("error: brew not found");
  await captureDocument(document, {
    name: "app-setup-script-settings-05-logs",
    viewport: { width: 1440, height: 900 },
    expectations: [
      "The Logs tab 'Log source' select shows 'App setup', with a level filter and search box below it.",
      "The feed lists the four script output lines, each prefixed with run '#1'.",
      "The 'error: brew not found' line is styled as an error and the 'warning:' line as a warning.",
    ],
  });
}, 60000);
