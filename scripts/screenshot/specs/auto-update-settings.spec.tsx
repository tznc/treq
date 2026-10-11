import * as React from "react";
import { expect, it } from "vitest";
import userEvent from "@testing-library/user-event";
import { createTestRepo, openRepo } from "../../../test/utils";
import { render, screen } from "../../../test/test-utils";
import { Dashboard } from "../../../src/components/Dashboard";
import { useToast } from "../../../src/components/ui/toast";
import { captureDocument } from "../capture";

it("captures Check for updates on the Application settings tab", async () => {
  const { repoPath } = createTestRepo(false);
  openRepo(repoPath);

  const user = userEvent.setup();
  render(<Dashboard />);

  await user.click(await screen.findByLabelText("Settings"));
  await user.click(await screen.findByRole("tab", { name: /application/i }));
  await screen.findByRole("tab", { name: /application/i, selected: true });

  const checkButton = await screen.findByRole("button", {
    name: /check for updates/i,
  });
  expect(checkButton).toBeEnabled();

  await captureDocument(document, {
    name: "auto-update-01-settings-check-button",
    expectations: [
      "The Settings page Application tab is open.",
      "An Updates section is visible with a 'Check for updates' outline button.",
      "Helper copy says it checks treq.dev for a newer signed release.",
    ],
  });

  await user.click(checkButton);
  // The test bridge has no updater plugin, so the manual check reports an error.
  await screen.findByText("Update check failed");

  await captureDocument(document, {
    name: "auto-update-02-check-failed-toast",
    expectations: [
      "A toast titled 'Update check failed' is visible in the bottom-left.",
    ],
  });
}, 60000);

function UpdateToastHarness() {
  const { addToast } = useToast();

  return (
    <button
      type="button"
      onClick={() => {
        addToast({
          type: "info",
          title: "Update available: v0.1.4",
          description: "You're on v0.1.3. Install the latest release?",
          action: {
            label: "Install and restart",
            onClick: () => {},
          },
        });
      }}
    >
      Show update toast
    </button>
  );
}

it("captures the actionable update-available toast", async () => {
  const user = userEvent.setup();
  render(<UpdateToastHarness />);

  await user.click(
    await screen.findByRole("button", { name: "Show update toast" }),
  );
  await screen.findByText("Update available: v0.1.4");
  await screen.findByRole("button", { name: "Install and restart" });

  await captureDocument(document, {
    name: "auto-update-03-install-toast",
    expectations: [
      "An info toast titled 'Update available: v0.1.4' is visible in the bottom-left.",
      "The toast includes an underlined 'Install and restart' action link under the description.",
      "A close (X) control is present on the toast so the prompt can be dismissed.",
    ],
  });
}, 30000);
