// @include-parallel
import * as React from "react";
import { act } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { Dashboard } from "../../src/components/Dashboard";
import { createWorkspace } from "../../src/lib/api";
import type { GpuiNativeEvent } from "../../src/lib/gpui-native";
import type {
  GpuiTerminalRoute,
  GpuiWorkspaceScreen,
} from "../../src/lib/gpui-screen";
import { useRemoteMutationFeedback } from "../../src/lib/remote-mutation-ui";
import { clearLastOpenedRemoteRepository } from "../../src/lib/remote-repository";
import { useRemoteCutoffStore } from "../../src/stores/remoteCutoffStore";
import {
  findGpuiScreen,
  gpuiScreen,
  sendGpuiScreenAction,
} from "../gpui-screen";
import { openSavedRemoteRepo } from "../remote-repo";
import { emitOnTestBus } from "../test-event-bus";
import { render, screen } from "../test-utils";
import { createTestRepo } from "../utils";

function sendNativeEvent(event: GpuiNativeEvent) {
  act(() => {
    emitOnTestBus("gpui-native-event", event);
  });
}

async function findRemoteWorkspace(
  check?: (screen: GpuiWorkspaceScreen) => void,
): Promise<GpuiWorkspaceScreen> {
  return findGpuiScreen("workspace", (s) => {
    expect(s.remoteChrome).not.toBeNull();
    check?.(s);
  });
}

describe("GPUI workspace view for a remote repository", () => {
  let repoPath: string;

  beforeEach(async () => {
    window.__TREQ_REACT_REMOTE_WORKSPACE__ = false;
    window.history.replaceState({}, "", "/");
    await clearLastOpenedRemoteRepository();
    useRemoteMutationFeedback.setState({
      ambiguousReason: null,
      lastStatus: null,
    });
    useRemoteCutoffStore.setState({ cutoffs: {} });
    ({ repoPath } = createTestRepo(false));
    await createWorkspace(repoPath, "feat/remote-ui");
    await openSavedRemoteRepo(repoPath);
  });

  afterEach(() => {
    window.__TREQ_REACT_TERMINAL__ = true;
  });

  it("hands GPUI the repository, its strip and its notice", async () => {
    render(<Dashboard />);
    const workspace = await findRemoteWorkspace((s) => {
      expect(s.remoteChrome?.state).toBe("online");
    });
    expect(workspace.repoPath).toBe(repoPath);
    expect(workspace.remoteChrome).toMatchObject({
      terminal: true,
      cutOff: false,
    });
    expect(workspace.remoteChrome?.notices.join(" ")).toContain(
      "Merging a remote workspace",
    );
    const snapshot = gpuiScreen() as unknown as {
      repository: {
        canonicalPath: string;
        cacheKey: string;
        endpointId: string;
        endpoint: { id: string } | null;
      };
    };
    expect(snapshot.repository).toMatchObject({
      canonicalPath: repoPath,
      endpointId: "endpoint-test",
      endpoint: { id: "endpoint-test" },
    });
    expect(snapshot.repository.cacheKey).toContain(repoPath);
  });

  it("opens the remote terminal dialog from the strip", async () => {
    render(<Dashboard />);
    await findRemoteWorkspace();
    sendGpuiScreenAction({ kind: "press", id: "remote.terminal" });
    expect(await screen.findByText("Remote terminal")).toBeTruthy();
  });

  it("dims the strip once GPUI reports the endpoint cut off", async () => {
    render(<Dashboard />);
    await findRemoteWorkspace();
    sendNativeEvent({
      kind: "remoteCutoff",
      endpointId: "endpoint-test",
      reason: "session_ended",
    });
    await findRemoteWorkspace((s) => {
      expect(s.remoteChrome).toMatchObject({ state: "cutoff", cutOff: true });
    });
    expect(screen.getByTestId("remote-cutoff-overlay")).toBeTruthy();
  });

  it("reports GPUI's remote changes as React's own", async () => {
    render(<Dashboard />);
    await findRemoteWorkspace();
    sendNativeEvent({
      kind: "remoteMutation",
      status: "ambiguous",
      reason: "the VM did not answer",
    });
    expect(useRemoteMutationFeedback.getState()).toMatchObject({
      lastStatus: "ambiguous",
      ambiguousReason: "the VM did not answer",
    });
  });

  it("opens new shells on the remote host in GPUI's pane", async () => {
    window.__TREQ_REACT_TERMINAL__ = false;
    render(<Dashboard />);
    const workspace = await findRemoteWorkspace((s) => {
      expect(s.terminal?.remoteShell).toBe(true);
    });
    expect(screen.queryByTestId("workspace-terminal-pane")).toBeNull();
    expect(workspace.terminal).not.toBeNull();
    sendGpuiScreenAction({
      kind: "field",
      field: "terminal.newRemoteShell",
      value: repoPath as unknown as Record<string, unknown>,
    });
    const terminal = (
      await findRemoteWorkspace((s) => {
        expect(s.terminal?.commands.at(-1)?.kind).toBe("openRemote");
      })
    ).terminal as GpuiTerminalRoute;
    expect(terminal.commands.at(-1)?.remote).toMatchObject({
      endpoint: { id: "endpoint-test" },
      repositoryId: repoPath,
      workspaceId: "root",
      remoteWorkingDirectory: repoPath,
      launch: { type: "shell" },
      reattach: false,
    });
    expect(terminal.commands.at(-1)?.remote?.label).toMatch(/^shell-/);
  });
});
