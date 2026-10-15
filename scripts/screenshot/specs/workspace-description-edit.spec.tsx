import userEvent from "@testing-library/user-event";
import * as React from "react";
import { expect, it } from "vitest";
import { Dashboard } from "../../../src/components/Dashboard";
import { createWorkspace, updateWorkspace } from "../../../src/lib/api";
import { render, screen, within } from "../../../test/test-utils";
import {
  createTestRepo,
  openRepo,
} from "../../../test/utils";
import { captureDocument } from "../capture";
import { clickGpuiRow } from "../../../test/gpui-sidebar";

// Scenario: the user opens a workspace whose description an agent wrote,
// edits it in place from the card's pencil button, and saves.
it("captures editing the workspace description in the Code tab", async () => {
  const { repoPath } = createTestRepo(false);
  openRepo(repoPath);
  const id = await createWorkspace(repoPath, "feat/login-limits");
  await updateWorkspace(
    repoPath,
    id,
    undefined,
    "Rate-limit login",
    "## Summary\n\n- Rate-limit the login endpoint\n\n## Testing\n- cargo test",
  );

  const user = userEvent.setup();
  render(<Dashboard />);
  await clickGpuiRow("feat/login-limits");
  const panel = await screen.findByTestId("workspace-description");
  await within(panel).findByRole("heading", { name: "Summary" });
  await captureDocument(document, {
    name: "workspace-description-edit-01-view",
    expectations: [
      "The description card sits between the prompt input and the file list and renders 'Summary' and 'Testing' as headings with bullets.",
      "A small pencil icon button sits in the card's top-right corner.",
    ],
  });

  await user.click(
    within(panel).getByRole("button", { name: "Edit workspace description" }),
  );
  const textarea = within(panel).getByRole("textbox", {
    name: "Workspace description",
  });
  await user.type(textarea, "{Enter}- Add a lockout test");
  expect(textarea).toHaveValue(
    "## Summary\n\n- Rate-limit the login endpoint\n\n## Testing\n- cargo test\n- Add a lockout test",
  );
  await captureDocument(document, {
    name: "workspace-description-edit-02-editing",
    expectations: [
      "The card now shows a monospace textarea holding the raw Markdown, tall enough to show every line with no inner scrollbar.",
      "Cancel and Save buttons sit right-aligned under the textarea.",
    ],
  });

  await user.click(within(panel).getByRole("button", { name: "Save" }));
  await within(panel).findByText("Add a lockout test");
  await captureDocument(document, {
    name: "workspace-description-edit-03-saved",
    expectations: [
      "The card is back to rendered Markdown, and the Testing list now has a second bullet 'Add a lockout test'.",
      "The pencil button is back in the top-right corner.",
    ],
  });
}, 60000);
