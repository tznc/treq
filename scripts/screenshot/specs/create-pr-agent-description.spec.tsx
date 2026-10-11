import userEvent from "@testing-library/user-event";
import * as React from "react";
import { expect, it, vi } from "vitest";
import { Dashboard } from "../../../src/components/Dashboard";
import { ghCreatePr, updateWorkspace } from "../../../src/lib/api";
import { render, screen, waitFor } from "../../../test/test-utils";
import { createTestRepo, openRepo } from "../../../test/utils";
import {
  findEnabledCreatePr,
  openWorkspace,
  setupPushedWorkspaceWithGitHub,
} from "../../../test/integration/workspace/create-pr-helpers";
import { captureDocument } from "../capture";

vi.mock("../../../src/lib/api", async () => {
  const actual = await vi.importActual<typeof import("../../../src/lib/api")>(
    "../../../src/lib/api",
  );
  return {
    ...actual,
    getCachedPrInfo: vi.fn().mockResolvedValue(null),
    getPrInfoViaGh: vi.fn().mockResolvedValue(null),
    startPrStatusPolling: vi.fn(async () => undefined),
    refreshPrBranchStatus: vi.fn(async () => undefined),
    stopPrStatusPolling: vi.fn(async () => undefined),
    refreshPrStatuses: vi.fn(async () => undefined),
    ghCreatePr: vi.fn().mockResolvedValue(81),
  };
});

const AGENT_BODY =
  "## Summary\n\n- Rate-limit the login endpoint\n\n## Testing\n- cargo test";

// Scenario: the workspace view is already open when the agent rewrites the
// description with `treq desc -f -`. One click on Create PR must send the
// agent's text as the PR body, not the description the view loaded with.
it("captures Create PR sending the description an agent just wrote", async () => {
  const { repoPath } = createTestRepo(true);
  openRepo(repoPath);
  const { workspace } = await setupPushedWorkspaceWithGitHub(repoPath, {
    description: "Initial task from the user",
  });

  const user = userEvent.setup();
  render(<Dashboard />);

  const header = await openWorkspace(user, "feat/create-pr");
  const createPr = await findEnabledCreatePr(header);
  await updateWorkspace(
    repoPath,
    workspace.id,
    undefined,
    undefined,
    AGENT_BODY,
  );
  await captureDocument(document, {
    name: "create-pr-agent-description-01-before",
    expectations: [
      "The workspace header for feat/create-pr shows an enabled dark 'Create PR' button with a GitHub icon.",
      "The description box in the Code tab, between the prompt input and the file list, still reads 'Initial task from the user': the view has not re-read the description the agent just wrote.",
    ],
  });

  await user.click(createPr);
  await screen.findByText("Pull request created");
  await waitFor(() =>
    expect(ghCreatePr).toHaveBeenCalledWith(
      "acme/treq",
      expect.any(String),
      AGENT_BODY,
      expect.any(String),
      "feat/create-pr",
      false,
    ),
  );
  await captureDocument(document, {
    name: "create-pr-agent-description-02-created",
    expectations: [
      "A green success toast reads 'Pull request created' with '#81' beneath it.",
      "The description box in the Code tab now shows the agent's description rendered as Markdown: 'Summary' and 'Testing' headings with bullet lists, no literal '##'.",
    ],
  });
}, 60000);
