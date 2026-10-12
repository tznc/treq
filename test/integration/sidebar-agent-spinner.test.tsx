// @include-parallel
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { Dashboard } from "../../src/components/Dashboard";
import { createWorkspace } from "../../src/lib/api";
import { installFakeAgents } from "../fake-agent";
import { render, screen, waitFor } from "../test-utils";
import { createTestRepo, findSidebarBranchElement, openRepo } from "../utils";

describe("sidebar agent spinner", () => {
  let restoreAgents: () => void;
  let user: ReturnType<typeof userEvent.setup>;

  beforeEach(() => {
    restoreAgents = installFakeAgents();
    user = userEvent.setup();
  });

  afterEach(() => {
    restoreAgents();
  });

  it("spins on a clean workspace while its agent works, and stops when it goes idle", async () => {
    const { repoPath } = createTestRepo(false);
    openRepo(repoPath);
    const workspaceId = await createWorkspace(repoPath, "feat/clean-agent");
    render(<Dashboard />);

    await user.click(await findSidebarBranchElement("feat/clean-agent"));
    await screen.findByTestId("workspace-terminal-pane");
    expect(
      screen.queryByTestId(`workspace-status-indicator-${workspaceId}`),
    ).not.toBeInTheDocument();

    await user.keyboard("{Meta>}]{/Meta}");

    const spinner = await screen.findByTestId(
      `workspace-status-indicator-${workspaceId}`,
      {},
      { timeout: 15000 },
    );
    expect(spinner).toHaveAttribute("aria-label", "Agent working");

    await waitFor(
      () =>
        expect(
          screen.queryByTestId(`workspace-status-indicator-${workspaceId}`),
        ).not.toBeInTheDocument(),
      { timeout: 15000 },
    );
  }, 60000);
});
