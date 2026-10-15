// @include-parallel
import { describe, expect, it, beforeEach } from "vitest";
import userEvent from "@testing-library/user-event";
import {
  createTestRepo,
  openRepo,
  resolveWorkspacePath,
  writeWorkspaceFile,
} from "../utils";
import {
  createWorkspace,
  getWorkspaces,
  listStashes,
  stashWorkspaceChanges,
} from "../../src/lib/api";
import { render, screen, waitFor, within } from "../test-utils";
import { Dashboard } from "../../src/components/Dashboard";
import { clickGpuiRow } from "../gpui-sidebar";

describe("stash dual-pane modal", () => {
  let user: ReturnType<typeof userEvent.setup>;
  let repoPath: string;
  let branchName: string;

  beforeEach(async () => {
    branchName = "feat/stash-int";
    const { repoPath: path } = createTestRepo(false);
    repoPath = path;
    openRepo(repoPath);
    user = userEvent.setup();

    const workspaceId = await createWorkspace(repoPath, branchName);
    const workspace = (await getWorkspaces(repoPath)).find(
      (w) => w.id === workspaceId,
    );
    if (!workspace) throw new Error("workspace missing");

    writeWorkspaceFile(
      resolveWorkspacePath(repoPath, workspace.workspace_path),
      "int-stash.txt",
      "hello stash\n",
    );
    await stashWorkspaceChanges(repoPath, workspaceId);
    expect(await listStashes(repoPath)).toHaveLength(1);
  });

  it("opens from the command palette and shows list + isolated diff", async () => {
    render(<Dashboard />);
    await clickGpuiRow(branchName);
    await screen.findByTestId("show-workspace-header");

    await user.keyboard("{Meta>}k{/Meta}");
    const cmdk = await screen.findByTestId("modal");
    await user.type(
      within(cmdk).getByPlaceholderText(/type a command/i),
      "stashed",
    );
    await user.click(await within(cmdk).findByText("View Stashed Changes"));

    const modal = await screen.findByTestId("stash-modal");
    const list = await within(modal).findByTestId("stash-list");
    expect(within(list).getByText(branchName)).toBeTruthy();
    await waitFor(() =>
      expect(within(modal).getByTestId("stash-diff-pane")).toBeTruthy(),
    );
    expect(within(modal).getByText("int-stash.txt")).toBeTruthy();
  });
});
