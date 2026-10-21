import * as fs from "node:fs";
import * as path from "node:path";
import * as React from "react";
import { expect, it } from "vitest";
import userEvent from "@testing-library/user-event";
import {
  createTestRepo,
  openRepo,
} from "../../../test/utils";
import { render, screen, waitFor, within } from "../../../test/test-utils";
import { Dashboard } from "../../../src/components/Dashboard";
import {
  createWorkspace,
  getWorkspaces,
  setWorkspaceTargetBranch,
} from "../../../src/lib/api";
import { getFullWorkspacePath } from "../../../src/lib/utils";
import { captureDocument } from "../capture";
import { stackGpui } from "../../../test/gpui-sidebar";

const EXISTING_BRANCH = "treq/existing-work";
const TITLE = "Settings Dark Mode";
const EXPECTED_BRANCH = "treq/settings-dark-mode";

// Scenario: end-to-end review of creating a workspace from the home repo via
// the "Stack" dialog. The home repo has one uncommitted file so the dialog's
// Changes panel has something to show, and one pre-existing workspace so the
// "branch already exists" warning can be exercised.
it("captures the workspace creation flow from the home repo", async () => {
  const { repoPath } = createTestRepo(false);
  openRepo(repoPath);

  // Incidental background state, not part of the flow under test.
  await createWorkspace(repoPath, EXISTING_BRANCH);
  fs.writeFileSync(
    path.join(repoPath, "dark-mode.css"),
    "body { background: #111; }\n",
  );

  const user = userEvent.setup();
  render(<Dashboard />);

  await screen.findByTestId("show-workspace-header");
  const stackButton = await screen.findByRole("button", { name: "Stack" });
  await captureDocument(document, {
    name: "workspace-creation-flow-01-home",
    expectations: [
      "The home repo view is shown with a 'Stack' button in the header.",
      "The sidebar lists the pre-existing 'treq/existing-work' workspace.",
    ],
  });

  await user.click(stackButton);
  const dialog = await screen.findByTestId("modal");
  await within(dialog).findByText("Stack a new Workspace");
  const submit = within(dialog).getByRole("button", {
    name: "Create Workspace",
  });
  expect(submit).toBeDisabled();
  const moveToggle = within(dialog).getByRole("button", {
    name: "Move to workspace",
  });
  expect(moveToggle).toHaveAttribute("aria-expanded", "false");
  await captureDocument(document, {
    name: "workspace-creation-flow-02-dialog-empty",
    expectations: [
      "Under 'Stacking On', a stack card shows a highlighted new-workspace row holding an empty branch input (placeholder 'treq/example'), above an arrow to the default branch.",
      "There is no separate Branch Name field below Description/Advanced; a collapsed 'Move to workspace' row sits below the fields.",
      "The 'Create Workspace' button is disabled (greyed) because no branch name is set.",
    ],
  });

  await user.type(within(dialog).getByLabelText("Title (optional)"), TITLE);
  await user.type(
    within(dialog).getByLabelText("Description (optional)"),
    "Add dark mode to settings.{Enter}Cover the header, sidebar and dialogs.{Enter}Respect the OS theme by default.{Enter}Persist the choice per user.",
  );
  const branchInput = within(dialog).getByLabelText("Branch Name");
  await waitFor(() => expect(branchInput).toHaveValue(EXPECTED_BRANCH));
  await waitFor(() => expect(submit).toBeEnabled());
  // Let the debounced branch-exists check settle.
  await new Promise((resolve) => setTimeout(resolve, 800));
  await captureDocument(document, {
    name: "workspace-creation-flow-03-filled",
    expectations: [
      `The branch input inside the stack card's new-workspace row is auto-filled with '${EXPECTED_BRANCH}' from the title, with a green check.`,
      "The Description box has grown to show all four typed lines with no scrollbar or clipped text.",
      "The 'Create Workspace' button is enabled.",
    ],
  });

  await user.click(moveToggle);
  await within(dialog).findByRole("tab", { name: /^Changes/ });
  await user.click(within(dialog).getByRole("tab", { name: /^Changes/ }));
  await within(dialog).findByText("dark-mode.css");
  await captureDocument(document, {
    name: "workspace-creation-flow-03b-move-expanded",
    expectations: [
      "The 'Move to workspace' row now has a down chevron and shows Commits/Changes tabs beneath the form fields, full dialog width.",
      "The Changes tab is active and lists dark-mode.css.",
      "The footer (Cancel / Create Workspace) is still visible; the body scrolls if content is taller than the viewport.",
    ],
  });
  await user.click(moveToggle);

  await user.click(within(dialog).getByRole("button", { name: "Advanced" }));
  await user.type(
    await within(dialog).findByLabelText(/Sparse paths/),
    "src/settings{Enter}src/theme{Enter}src/components/ui",
  );
  await captureDocument(document, {
    name: "workspace-creation-flow-04-advanced",
    expectations: [
      "The Advanced section is expanded; the Sparse paths box has grown to show all three typed lines.",
      "The dialog still fits the viewport without clipped fields or footer buttons.",
    ],
  });

  await user.clear(branchInput);
  await user.type(branchInput, EXISTING_BRANCH);
  await within(dialog).findByText(
    `A workspace for ${EXISTING_BRANCH} already exists`,
  );
  await captureDocument(document, {
    name: "workspace-creation-flow-05-branch-exists",
    expectations: [
      `Inside the stack card's new-workspace row, red text reads 'A workspace for ${EXISTING_BRANCH} already exists' under the input.`,
    ],
  });

  await user.clear(branchInput);
  await user.type(branchInput, EXPECTED_BRANCH);
  await within(dialog).findByText("Stack a new Workspace");
  await waitFor(() =>
    expect(
      within(dialog).queryByTestId("branch-name-error"),
    ).not.toBeInTheDocument(),
  );
  await user.click(submit);
  await waitFor(() => {
    expect(screen.queryByTestId("modal")).not.toBeInTheDocument();
  });

  const header = await screen.findByTestId("show-workspace-header");
  await within(header).findByText(EXPECTED_BRANCH);
  await new Promise((resolve) => setTimeout(resolve, 500));
  await captureDocument(document, {
    name: "workspace-creation-flow-06-created",
    expectations: [
      `The workspace header shows the new branch '${EXPECTED_BRANCH}' (or its title '${TITLE}').`,
      "The sidebar lists the new workspace alongside 'treq/existing-work', with the new one selected.",
      "A success toast or no error message is visible; nothing indicates failure.",
    ],
  });
}, 90000);

// Scenario: stacking a new workspace onto an existing two-level stack from
// the sidebar's "Stack a workspace" action. Checks the stack card places the
// new row above the parent for "after" and below it for "before", and that
// the branch name is typed straight into that row.
it("captures the stack card before/after positions in the stack dialog", async () => {
  const { repoPath } = createTestRepo(false);
  openRepo(repoPath);

  // Incidental background state: feat/base <- feat/api.
  await createWorkspace(repoPath, "feat/base");
  const parentId = await createWorkspace(repoPath, "feat/api");
  const parent = (await getWorkspaces(repoPath)).find(
    (workspace) => workspace.id === parentId,
  );
  if (!parent) throw new Error("feat/api workspace missing");
  await setWorkspaceTargetBranch(
    repoPath,
    getFullWorkspacePath(parent),
    parentId,
    "feat/base",
  );

  const user = userEvent.setup();
  render(<Dashboard />);

  await stackGpui("feat/api");
  const dialog = await screen.findByTestId("modal");
  const card = await within(dialog).findByTestId("new-workspace-stack-card");
  await within(card).findByText("feat/base");

  const branchInput = within(card).getByLabelText("Branch Name");
  await user.click(branchInput);
  await user.type(branchInput, "feat/api-client");
  await waitFor(() => expect(branchInput).toHaveValue("feat/api-client"));
  await new Promise((resolve) => setTimeout(resolve, 800));
  await captureDocument(document, {
    name: "workspace-creation-flow-07-stack-after",
    expectations: [
      "Position 'After' is selected; the stack card lists, top to bottom: the highlighted new row, feat/api, feat/base, then an arrow to the default branch.",
      "The new row's input contains the typed branch 'feat/api-client' with a green check.",
    ],
  });

  await user.click(within(dialog).getByRole("button", { name: /^before$/i }));
  await captureDocument(document, {
    name: "workspace-creation-flow-08-stack-before",
    expectations: [
      "Position 'Before' is selected; the order is now feat/api, the highlighted new row, feat/base, then the default branch.",
      "The new row still holds 'feat/api-client' in its input.",
    ],
  });
}, 90000);
