import * as React from "react";
import { it } from "vitest";
import userEvent from "@testing-library/user-event";
import { createTestRepo, openRepo } from "../../../test/utils";
import { createWorkspace } from "../../../src/lib/api";
import { render, screen, waitFor } from "../../../test/test-utils";
import { Dashboard } from "../../../src/components/Dashboard";
import { captureDocument } from "../capture";
import { clickGpuiRow, sendGpuiAction } from "../../../test/gpui-sidebar";

it("captures Copilot as a selectable agent in settings and the task input toolbar", async () => {
  const { repoPath } = createTestRepo(false);
  openRepo(repoPath);
  await createWorkspace(repoPath, "feat/copilot-agent-option");

  const user = userEvent.setup();
  render(<Dashboard />);

  sendGpuiAction({ kind: "openSettings" });
  await screen.findByText("Settings");
  const applicationTab = await screen.findByRole("tab", { name: /application/i });
  await user.click(applicationTab);

  const agentSelect = await screen.findByLabelText(/default agent/i);
  await user.selectOptions(agentSelect, "copilot");
  await waitFor(() => expect(agentSelect).toHaveValue("copilot"));

  await captureDocument(document, {
    name: "copilot-agent-option-01-settings-dropdown",
    expectations: [
      "The 'Default Agent' select in the Application settings tab shows 'Copilot' selected.",
      "The dropdown's options include Claude, Codex, Cursor, and Copilot.",
    ],
  });

  await user.click(
    await screen.findByRole("button", { name: /save settings/i }),
  );
  await screen.findByText("Settings Saved");

  await user.click(await screen.findByRole("button", { name: "Close" }));

  await clickGpuiRow("feat/copilot-agent-option");
  const agentPicker = await screen.findByLabelText("Agent");
  await waitFor(() => expect(agentPicker).toHaveValue("copilot"));

  await captureDocument(document, {
    name: "copilot-agent-option-02-task-toolbar",
    expectations: [
      "The task input toolbar's agent picker shows 'Copilot' selected with its brand icon next to it.",
      "The Plan button is hidden since Copilot, like Codex, has no plan mode.",
    ],
  });
}, 60000);
