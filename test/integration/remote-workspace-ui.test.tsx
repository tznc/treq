// @include-parallel
import * as React from "react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it } from "vitest";
import { Dashboard } from "../../src/components/Dashboard";
import {
  createCommit,
  createWorkspace,
  ensureWorkspaceIndexed,
  getWorkspaceChangedFiles,
  getWorkspaces,
} from "../../src/lib/api";
import { clearLastOpenedRemoteRepository } from "../../src/lib/remote-repository";
import { useRemoteMutationFeedback } from "../../src/lib/remote-mutation-ui";
import { useRemoteCutoffStore } from "../../src/stores/remoteCutoffStore";
import { act, render, screen, waitFor, within } from "../test-utils";
import {
  createTestRepo,
  findSidebarBranchElement,
  newCommitWithParents,
  resolveChangeId,
  resolveWorkspacePath,
  writeWorkspaceFile,
} from "../utils";
import { openSavedRemoteRepo } from "../remote-repo";

describe("remote workspace UI", () => {
  let user: ReturnType<typeof userEvent.setup>;

  beforeEach(async () => {
    user = userEvent.setup();
    window.history.replaceState({}, "", "/");
    await clearLastOpenedRemoteRepository();
    useRemoteMutationFeedback.setState({
      ambiguousReason: null,
      lastStatus: null,
    });
    useRemoteCutoffStore.setState({ cutoffs: {} });
  });

  it("renders the normal workspace tree for a remote repository", async () => {
    const { repoPath, defaultBranch } = createTestRepo(false);
    await createWorkspace(repoPath, "feat/remote-ui");
    await openSavedRemoteRepo(repoPath);

    render(React.createElement(Dashboard));

    expect(await screen.findByTestId("show-workspace-header")).toBeTruthy();
    expect(screen.queryByText("Remote repository connected")).toBeNull();
    expect(screen.queryByText("Remote review")).toBeNull();
    expect(await findSidebarBranchElement("feat/remote-ui")).toBeTruthy();
    expect(
      await within(
        await screen.findByTestId("show-workspace-header"),
      ).findByRole("button", { name: defaultBranch }),
    ).toBeTruthy();
    expect(screen.getByRole("button", { name: "Refresh" })).toBeTruthy();
    const notice = screen.getByTestId("remote-capability-notice");
    expect(notice).toHaveTextContent("Merging a remote workspace");
    expect(notice).not.toHaveTextContent("native SSH PTY");
  });

  it("selects a remote workspace from the sidebar", async () => {
    const { repoPath } = createTestRepo(false);
    await createWorkspace(repoPath, "feat/select-me");
    await openSavedRemoteRepo(repoPath);

    render(React.createElement(Dashboard));
    await user.click(await findSidebarBranchElement("feat/select-me"));

    const header = await screen.findByTestId("show-workspace-header");
    expect(await within(header).findByText("feat/select-me")).toBeTruthy();
  });

  it("shows remote changes, commits, and available mutations", async () => {
    const { repoPath } = createTestRepo(false);
    const workspaceId = await createWorkspace(repoPath, "feat/changes");
    const workspace = (await getWorkspaces(repoPath)).find(
      (item) => item.id === workspaceId,
    );
    if (!workspace) throw new Error("workspace missing");
    writeWorkspaceFile(
      resolveWorkspacePath(repoPath, workspace.workspace_path),
      "remote-change.txt",
      "hello remote\n",
    );
    await openSavedRemoteRepo(repoPath);

    render(React.createElement(Dashboard));
    await user.click(await findSidebarBranchElement("feat/changes"));
    await user.click(await screen.findByRole("tab", { name: /^Changes/ }));

    expect(
      (await screen.findAllByText("remote-change.txt")).length,
    ).toBeGreaterThan(0);
    await user.click(await screen.findByTitle("remote-change.txt"));
    expect(await screen.findByText(/hello remote/)).toBeTruthy();
    await user.click(await screen.findByRole("tab", { name: /^Commits/ }));
    const notice = screen.getByTestId("remote-capability-notice");
    expect(notice).toHaveTextContent("Merging a remote workspace");
    expect(notice).not.toHaveTextContent("native SSH PTY");
  });

  it("shows remote conflicts in the workspace review UI", async () => {
    const { repoPath } = createTestRepo(false);
    const workspaceId = await createWorkspace(repoPath, "feat/conflict");
    const workspace = (await getWorkspaces(repoPath)).find(
      (item) => item.id === workspaceId,
    );
    if (!workspace) throw new Error("workspace missing");
    const workspacePath = resolveWorkspacePath(
      repoPath,
      workspace.workspace_path,
    );

    writeWorkspaceFile(workspacePath, "README.md", "workspace side\n");
    await createCommit(repoPath, workspaceId, "workspace conflicting change");
    const workspaceChangeId = resolveChangeId(workspacePath, "@-");

    writeWorkspaceFile(repoPath, "README.md", "main side\n");
    await createCommit(repoPath, null, "main conflicting change");
    const mainChangeId = resolveChangeId(repoPath, "@-");

    newCommitWithParents(workspacePath, [workspaceChangeId, mainChangeId]);
    await ensureWorkspaceIndexed(repoPath, workspaceId, workspacePath);
    await openSavedRemoteRepo(repoPath);

    render(React.createElement(Dashboard));
    await user.click(await findSidebarBranchElement("feat/conflict"));
    await user.click(await screen.findByRole("tab", { name: /^Changes/ }));

    expect(
      await screen.findByRole("button", { name: "Conflicts" }),
    ).toBeTruthy();
    expect((await screen.findAllByTitle("README.md")).length).toBeGreaterThan(
      0,
    );
  }, 20_000);

  it("enables shell and agent actions for remote workspaces", async () => {
    const { repoPath } = createTestRepo(false);
    await createWorkspace(repoPath, "feat/caps");
    await openSavedRemoteRepo(repoPath);

    render(React.createElement(Dashboard));
    const row = (await findSidebarBranchElement("feat/caps")).closest(
      "div",
    ) as HTMLElement;
    const shell = within(row).getByRole("button", { name: "Open shell" });
    expect(shell).toBeEnabled();
    expect(shell).not.toHaveAttribute("title");
    const agent = within(row).getByRole("button", { name: "Start agent" });
    expect(agent).toBeEnabled();
    const notice = screen.getByTestId("remote-capability-notice");
    expect(notice).toHaveTextContent("Merging a remote workspace");
    expect(notice).not.toHaveTextContent("native SSH PTY");
  });

  it("blocks interaction behind a credential cutoff banner", async () => {
    const { repoPath } = createTestRepo(false);
    await openSavedRemoteRepo(repoPath, { endpointId: "ep-cut" });

    render(React.createElement(Dashboard));
    await screen.findByTestId("show-workspace-header");

    act(() => {
      useRemoteCutoffStore.getState().recordCutoff("ep-cut", "key_revoked");
    });

    const banner = await screen.findByTestId("remote-status-banner");
    expect(banner).toHaveAttribute("data-state", "cutoff");
    expect(banner).toHaveTextContent("reauthenticate");
    expect(screen.getByTestId("remote-cutoff-overlay")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Refresh" })).toBeDisabled();
  });

  it("explains an ambiguous mutation without retrying", async () => {
    const { repoPath } = createTestRepo(false);
    await openSavedRemoteRepo(repoPath);

    render(React.createElement(Dashboard));
    await screen.findByTestId("show-workspace-header");

    act(() => {
      useRemoteMutationFeedback.getState().report({
        status: "ambiguous",
        reason: "Could not tell whether the remote commit landed.",
      });
    });

    const dialog = await screen.findByRole("dialog", {
      name: "Remote change could not be verified",
    });
    expect(screen.getByTestId("remote-ambiguous-reason")).toHaveTextContent(
      "Could not tell whether the remote commit landed.",
    );

    await userEvent.click(
      within(dialog).getByRole("button", { name: "Refresh" }),
    );
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    await screen.findByTestId("show-workspace-header");
  });

  it("isolates cache identity across endpoint generations", async () => {
    const { repoPath } = createTestRepo(false);
    await createWorkspace(repoPath, "feat/gen-a");
    await openSavedRemoteRepo(repoPath, {
      endpointId: "ep-a",
      generation: 1,
    });

    const { unmount } = render(React.createElement(Dashboard));
    expect(await findSidebarBranchElement("feat/gen-a")).toBeTruthy();
    unmount();

    await openSavedRemoteRepo(repoPath, {
      endpointId: "ep-a",
      generation: 2,
    });
    render(React.createElement(Dashboard));
    expect(await findSidebarBranchElement("feat/gen-a")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Refresh" })).toBeTruthy();
  });

  it("refreshes when the workspace change marker advances", async () => {
    const { repoPath } = createTestRepo(false);
    const workspaceId = await createWorkspace(repoPath, "feat/marker");
    const workspace = (await getWorkspaces(repoPath)).find(
      (item) => item.id === workspaceId,
    );
    if (!workspace) throw new Error("workspace missing");
    await openSavedRemoteRepo(repoPath);

    render(React.createElement(Dashboard));
    await user.click(await findSidebarBranchElement("feat/marker"));
    await user.click(await screen.findByRole("tab", { name: /^Changes/ }));
    await screen.findByText("No changes to review");

    writeWorkspaceFile(
      resolveWorkspacePath(repoPath, workspace.workspace_path),
      "foreign.txt",
      "from another client\n",
    );
    await getWorkspaceChangedFiles(repoPath, workspaceId);

    await waitFor(
      () => {
        expect(screen.getAllByText("foreign.txt").length).toBeGreaterThan(0);
      },
      { timeout: 8_000 },
    );
  }, 15_000);
});
