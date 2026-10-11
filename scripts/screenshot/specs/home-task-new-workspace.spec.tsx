import * as React from "react";
import userEvent from "@testing-library/user-event";
import { expect, it } from "vitest";
import { Dashboard } from "../../../src/components/Dashboard";
import { installFakeAgents } from "../../../test/fake-agent";
import { render, screen, waitFor, within } from "../../../test/test-utils";
import {
  createTestRepo,
  findSidebarBranchElement,
  openRepo,
} from "../../../test/utils";
import { captureDocument } from "../capture";

const PROMPT = "Add input validation to the signup form";
const BRANCH = "add-input-validation-to-the-signup-form";

it("captures a home-view task starting in its own workspace", async () => {
  const restoreAgents = installFakeAgents();
  try {
    const { repoPath } = createTestRepo(false);
    openRepo(repoPath);
    const user = userEvent.setup();
    render(<Dashboard />);

    const toggle = await screen.findByRole("switch", {
      name: "Run in a new workspace",
    });
    expect(toggle).toBeChecked();
    await user.type(
      await screen.findByPlaceholderText("Describe a task..."),
      PROMPT,
    );

    await captureDocument(document, {
      name: "home-task-new-workspace-01-toggle-on",
      expectations: [
        "The home view's task input shows a compact 'Run in a new workspace' switch, turned on (blue), with its thumb contained in the track and aligned with the File and Attach buttons.",
        `The task input contains "${PROMPT}".`,
      ],
    });

    await user.click(screen.getByRole("button", { name: /^edit$/i }));
    await findSidebarBranchElement(BRANCH);
    const pane = await screen.findByTestId("workspace-terminal-pane");
    await within(pane).findByText(`prompt: ${PROMPT}`, {}, { timeout: 15000 });

    await captureDocument(document, {
      name: "home-task-new-workspace-02-session-in-new-workspace",
      expectations: [
        `A new '${BRANCH}' workspace is listed in the sidebar under Workspaces.`,
        `The agent terminal shows the fake agent's output, including 'prompt: ${PROMPT}', and its tab carries a badge naming the '${BRANCH}' workspace.`,
        `A 'Session started' toast says the session is in ${BRANCH}, while the page stays on the home repo.`,
      ],
    });

    await user.click(toggle);
    await waitFor(() => expect(toggle).not.toBeChecked());

    await captureDocument(document, {
      name: "home-task-new-workspace-03-toggle-off",
      expectations: [
        "The compact 'Run in a new workspace' switch is now off (grey), with its thumb contained in the track and its label still visible and aligned.",
      ],
    });
  } finally {
    restoreAgents();
  }
}, 120000);
