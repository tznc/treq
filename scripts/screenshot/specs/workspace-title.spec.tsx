import userEvent from "@testing-library/user-event";
import { it } from "vitest";
import { Dashboard } from "../../../src/components/Dashboard";
import { createWorkspace, updateWorkspace } from "../../../src/lib/api";
import { render, screen, within } from "../../../test/test-utils";
import {
  createTestRepo,
  findSidebarBranchElement,
  openRepo,
} from "../../../test/utils";
import { captureDocument } from "../capture";

it("captures the wrapping Code title and inline editor", async () => {
  const { repoPath } = createTestRepo(false);
  const id = await createWorkspace(repoPath, "feat/workspace-title");
  await updateWorkspace(
    repoPath,
    id,
    undefined,
    "Show the complete workspace title above its description, with an inline editor that remains comfortable in a narrow pane",
    "## Summary\n\nMove the title into the Code tab and let users edit it in place.",
  );
  openRepo(repoPath);
  render(<Dashboard />);
  const user = userEvent.setup();
  await user.click(await findSidebarBranchElement("feat/workspace-title"));
  const panel = await screen.findByTestId("workspace-title");
  await screen.findByTestId("workspace-description");
  await captureDocument(document, {
    name: "workspace-title-display",
    viewport: { width: 1100, height: 900 },
    expectations: [
      "The full title wraps above the description without ellipsis or clipping.",
      "A pencil icon appears alongside the title.",
      "The workspace header shows the branch without a second title row.",
    ],
  });
  await user.click(
    within(panel).getByRole("button", { name: "Edit workspace title" }),
  );
  await captureDocument(document, {
    name: "workspace-title-edit",
    viewport: { width: 1100, height: 900 },
    expectations: [
      "The full title appears in a wrapping textarea above the description.",
      "Save and Cancel buttons appear beneath the title editor.",
    ],
  });
});
