/**
 * Captures agent-terminal message queue: leftmost toolbar button (with count
 * badge when non-empty) and a dialog centered in the terminal body.
 */

import * as React from "react";
import { expect, it } from "vitest";
import userEvent from "@testing-library/user-event";
import {
  createTestRepo,
  findSidebarBranchElement,
  openRepo,
} from "../../../test/utils";
import { createWorkspace, getWorkspaces } from "../../../src/lib/api";
import { render, screen, waitFor, within } from "../../../test/test-utils";
import { Dashboard } from "../../../src/components/Dashboard";
import { captureDocument } from "../capture";

const BRANCH_NAME = "feat/agent-message-queue";

it("captures agent terminal message queue button and dialog", async () => {
  const { repoPath } = createTestRepo(false);
  openRepo(repoPath);

  await createWorkspace(repoPath, BRANCH_NAME);
  const workspaces = await getWorkspaces(repoPath);
  const workspace = workspaces.find((w) => w.branch_name === BRANCH_NAME);
  if (!workspace) throw new Error(`workspace ${BRANCH_NAME} not found`);

  const user = userEvent.setup();
  render(<Dashboard />);

  await user.click(await findSidebarBranchElement(workspace.branch_name));
  await screen.findByTestId("workspace-terminal-pane");

  await user.keyboard("{Meta>}]{/Meta}");

  const terminalPanel = await waitFor(() => {
    const el = document.querySelector('[data-terminal-id^="agent-"]');
    expect(el).not.toBeNull();
    return el as HTMLElement;
  });

  const queueButton = await within(terminalPanel).findByTestId(
    "agent-message-queue-button",
  );
  const toolbarButtons = within(
    queueButton.parentElement!.parentElement!,
  ).getAllByRole("button");
  expect(toolbarButtons[0]).toBe(queueButton);
  expect(
    screen.queryByTestId("agent-message-queue-composer"),
  ).not.toBeInTheDocument();

  await captureDocument(document, {
    name: "agent-message-queue-01-toolbar",
    expectations: [
      "Queue is the leftmost button in the agent terminal toolbar.",
      "No follow-up dialog is visible until the Queue button is clicked.",
    ],
  });

  await user.click(queueButton);
  const composer = await screen.findByTestId("agent-message-queue-composer");
  const emptyDialog = screen.getByTestId("agent-message-queue-dialog");
  expect(emptyDialog).toHaveAttribute("data-position", "terminal-center");
  expect(terminalPanel.contains(emptyDialog)).toBe(true);

  await captureDocument(document, {
    name: "agent-message-queue-02-empty-dialog",
    expectations: [
      "A themed dialog is centered in the terminal body with a follow-up input.",
      "No queued message rows are listed while the queue is empty.",
    ],
  });

  await user.type(composer, "Add unit tests for the queue{Enter}");
  await user.type(
    await screen.findByTestId("agent-message-queue-composer"),
    "Then update the docs{Enter}",
  );

  expect(
    await within(terminalPanel).findByTestId("agent-message-queue-count"),
  ).toHaveTextContent("2");

  await user.click(
    within(terminalPanel).getByTestId("agent-message-queue-button"),
  );
  await waitFor(() => {
    expect(
      screen.queryByTestId("agent-message-queue-dialog"),
    ).not.toBeInTheDocument();
  });

  await captureDocument(document, {
    name: "agent-message-queue-03-toolbar-badge",
    expectations: [
      "The Queue icon stays leftmost in the toolbar with a 2 count badge.",
      "No floating chip appears over the terminal body.",
    ],
  });

  await user.click(
    within(terminalPanel).getByTestId("agent-message-queue-button"),
  );
  const dialog = await screen.findByTestId("agent-message-queue-dialog");
  expect(dialog).toHaveAttribute("data-position", "terminal-center");
  expect(
    within(dialog).getByText("Add unit tests for the queue"),
  ).toBeInTheDocument();

  await captureDocument(document, {
    name: "agent-message-queue-04-dialog",
    expectations: [
      "The dialog is centered in the middle of the terminal with space above the bottom edge.",
      "Queued messages and the follow-up input are listed inside the dialog.",
    ],
  });
}, 60000);

it("hides the agent message queue when its feature preview is off", async () => {
  const { repoPath } = createTestRepo(false);
  openRepo(repoPath);

  await createWorkspace(repoPath, BRANCH_NAME);

  const user = userEvent.setup();
  render(<Dashboard />);

  await user.click(await screen.findByLabelText("Settings"));
  await user.click(await screen.findByRole("tab", { name: /feature preview/i }));
  const queueSwitch = await screen.findByRole("switch", {
    name: "Agent message queue",
  });
  await user.click(queueSwitch);
  expect(queueSwitch).toHaveAttribute("aria-checked", "false");

  await captureDocument(document, {
    name: "agent-message-queue-05-preview-setting-off",
    expectations: [
      "Feature Preview settings list an 'Agent message queue' row below 'Browser'.",
      "The 'Agent message queue' switch is off.",
    ],
  });

  await user.click(screen.getByRole("button", { name: "Close" }));
  await user.click(await findSidebarBranchElement(BRANCH_NAME));
  await screen.findByTestId("workspace-terminal-pane");
  await user.keyboard("{Meta>}]{/Meta}");

  const terminalPanel = await waitFor(() => {
    const el = document.querySelector('[data-terminal-id^="agent-"]');
    expect(el).not.toBeNull();
    return el as HTMLElement;
  });
  await within(terminalPanel).findByRole("button", { name: "Close session" });
  expect(
    within(terminalPanel).queryByTestId("agent-message-queue-button"),
  ).not.toBeInTheDocument();

  await captureDocument(document, {
    name: "agent-message-queue-06-toolbar-flag-off",
    expectations: [
      "The agent terminal toolbar has no Queue (list) icon left of the search button.",
      "The rest of the toolbar (search, close) renders normally.",
    ],
  });
}, 60000);
