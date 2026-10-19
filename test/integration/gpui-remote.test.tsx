// @include-parallel
import * as React from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { Dashboard } from "../../src/components/Dashboard";
import type { UserManagedFormValues } from "../../src/components/remote/remoteSetupLabels";
import type { RemoteTerminalTarget } from "../../src/components/RemoteTerminalPanel";
import { useGpuiRemoteDialogs } from "../../src/components/useGpuiRemoteDialogs";
import type { SshEndpoint } from "../../src/lib/api-types-remote";
import { type GpuiRemoteRoute, useGpuiScreen } from "../../src/lib/gpui-screen";
import { listUserManagedEndpoints } from "../../src/lib/remote-endpoints";
import { clearLastOpenedRemoteRepository } from "../../src/lib/remote-repository";
import { useRemoteMutationFeedback } from "../../src/lib/remote-mutation-ui";
import {
  findGpuiScreen,
  gpuiScreen,
  sendGpuiScreenAction,
} from "../gpui-screen";
import { openSavedRemoteRepo } from "../remote-repo";
import { act, fireEvent, render, screen, waitFor } from "../test-utils";
import { createTestRepo } from "../utils";

const press = (id: string) => sendGpuiScreenAction({ kind: "press", id });

const remote = (): GpuiRemoteRoute | undefined => gpuiScreen()?.remote;

async function findRemote(
  check: (remote: GpuiRemoteRoute) => void,
): Promise<GpuiRemoteRoute> {
  return waitFor(() => {
    const route = remote();
    if (!route) throw new Error("GPUI draws no remote dialog");
    check(route);
    return route;
  });
}

describe("GPUI remote dialogs", () => {
  beforeEach(async () => {
    window.__TREQ_REACT_REMOTE_DIALOGS__ = false;
    window.history.replaceState({}, "", "/");
    await clearLastOpenedRemoteRepository();
    useRemoteMutationFeedback.setState({
      ambiguousReason: null,
      lastStatus: null,
    });
  });

  it("draws the setup dialog over onboarding instead of React", async () => {
    render(React.createElement(Dashboard));
    await findGpuiScreen("onboarding");
    expect(remote()).toBeUndefined();

    sendGpuiScreenAction({ kind: "openRemote" });
    const route = await findRemote((r) => expect(r.setup).not.toBeNull());
    expect(route.setup).toMatchObject({
      allowUserManaged: true,
      registering: false,
      cloud: { instance: null, submitting: false },
    });
    expect(route.terminal).toBeNull();
    expect(route.ambiguous).toBeNull();
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(gpuiScreen()?.kind).toBe("onboarding");

    press("remote.setup.close");
    await waitFor(() => expect(remote()).toBeUndefined());
  });

  it("keeps React's dialog when the flag asks for it", async () => {
    window.__TREQ_REACT_REMOTE_DIALOGS__ = true;
    render(React.createElement(Dashboard));
    await findGpuiScreen("onboarding");

    sendGpuiScreenAction({ kind: "openRemote" });
    expect(
      await screen.findByRole("dialog", {
        name: "Connect a remote repository",
      }),
    ).toBeTruthy();
    await waitFor(() => expect(gpuiScreen()).toBeNull());
  });

  it("runs the cloud workspace flows GPUI hands back", async () => {
    render(React.createElement(Dashboard));
    await findGpuiScreen("onboarding");
    sendGpuiScreenAction({ kind: "openRemote" });
    await findRemote((r) => expect(r.setup).not.toBeNull());

    press("remote.cloud.provision");
    await findRemote((r) =>
      expect(r.setup?.cloud.provisioningStage).toBe(
        "Requesting provisioning...",
      ),
    );
  });

  it("registers the endpoint the form confirmed", async () => {
    render(React.createElement(Dashboard));
    await findGpuiScreen("onboarding");
    sendGpuiScreenAction({ kind: "openRemote" });
    await findRemote((r) => expect(r.setup).not.toBeNull());

    const values: UserManagedFormValues = {
      display_name: "Dev box",
      hostname: "devbox.example",
      port: 2222,
      username: "me",
      host_key_fingerprint: "SHA256:abc123",
      auth_identity_reference: "/home/me/.ssh/id_ed25519.pub",
      alias: null,
    };
    sendGpuiScreenAction({
      kind: "field",
      field: "remote.registerUserManaged",
      value: values as unknown as Record<string, unknown>,
    });

    await waitFor(async () => {
      const saved = await listUserManagedEndpoints();
      expect(saved).toEqual([expect.objectContaining(values)]);
    });
    await waitFor(() => expect(remote()?.setup ?? null).toBeNull());
  });

  it("draws the change that could not be verified, and clears it", async () => {
    render(React.createElement(Dashboard));
    await findGpuiScreen("onboarding");

    act(() => {
      useRemoteMutationFeedback.setState({
        ambiguousReason: "Connection reset while pushing",
      });
    });
    await findRemote((r) =>
      expect(r.ambiguous).toBe("Connection reset while pushing"),
    );
    expect(screen.queryByRole("dialog")).toBeNull();

    press("remote.ambiguous.dismiss");
    await waitFor(() => expect(remote()).toBeUndefined());
    expect(useRemoteMutationFeedback.getState().ambiguousReason).toBeNull();
  });

  it("refreshes the remote repository before it clears the change", async () => {
    render(React.createElement(Dashboard));
    await findGpuiScreen("onboarding");
    act(() => {
      useRemoteMutationFeedback.setState({ ambiguousReason: "Timed out" });
    });
    await findRemote((r) => expect(r.ambiguous).toBe("Timed out"));

    press("remote.ambiguous.refresh");
    await waitFor(() => expect(remote()).toBeUndefined());
    expect(useRemoteMutationFeedback.getState().ambiguousReason).toBeNull();
  });

  it("opens React's terminal dialog over a view GPUI does not draw", async () => {
    const { repoPath } = createTestRepo(false);
    await openSavedRemoteRepo(repoPath);
    render(React.createElement(Dashboard));

    await screen.findByTestId("show-workspace-header");
    fireEvent.click(
      await screen.findByRole("button", { name: "Terminal", hidden: true }),
    );
    expect(
      await screen.findByRole("dialog", { name: "Remote terminal" }),
    ).toBeTruthy();
    expect(remote()).toBeUndefined();
  });
});

const ENDPOINT = {
  id: "endpoint-1",
  instance_id: null,
  source: { type: "user_managed" },
  hostname: "devbox.example",
  port: 22,
  username: "me",
  host_keys: [],
  authentication: { type: "public_key", key_reference: "/home/me/.ssh/id" },
} as unknown as SshEndpoint;

function TerminalDialogOverScreen(props: {
  onOpenTarget: (target: RemoteTerminalTarget) => void;
  onClose: () => void;
}) {
  useGpuiScreen({ kind: "onboarding" }, () => {});
  useGpuiRemoteDialogs({
    setupOpen: false,
    allowUserManaged: true,
    cloudWorkspace: {
      instanceStatus: null,
      onProvision: async () => {},
      onWake: async () => {},
      onRepair: async () => {},
      onDelete: async () => {},
    },
    onCloseSetup: () => {},
    onRegisterUserManaged: async () => {},
    terminal: {
      endpoint: ENDPOINT,
      repositoryId: "/srv/project",
      workspaceId: "root",
      remoteWorkingDirectory: "/srv/project",
    },
    onCloseTerminal: props.onClose,
    onOpenTerminalTarget: props.onOpenTarget,
  });
  return null;
}

describe("GPUI remote terminal dialog", () => {
  beforeEach(() => {
    window.__TREQ_REACT_REMOTE_DIALOGS__ = false;
  });

  it("draws over a screen and opens the session GPUI picked", async () => {
    const targets: RemoteTerminalTarget[] = [];
    const onClose = vi.fn();
    render(
      <TerminalDialogOverScreen
        onOpenTarget={(target) => targets.push(target)}
        onClose={onClose}
      />,
    );
    await findGpuiScreen("onboarding");
    const route = await findRemote((r) => expect(r.terminal).not.toBeNull());
    expect(route.terminal).toEqual({
      endpoint: ENDPOINT,
      repositoryId: "/srv/project",
      workspaceId: "root",
    });

    sendGpuiScreenAction({
      kind: "field",
      field: "remote.terminal.open",
      value: { label: "claude-1", reattach: true },
    });
    sendGpuiScreenAction({
      kind: "field",
      field: "remote.terminal.open",
      value: { label: null, reattach: false },
    });
    expect(targets).toHaveLength(2);
    expect(targets[0]).toEqual({
      endpoint: ENDPOINT,
      repositoryId: "/srv/project",
      workspaceId: "root",
      remoteWorkingDirectory: "/srv/project",
      label: "claude-1",
      reattach: true,
    });
    expect(targets[1]).toMatchObject({ reattach: false });
    expect(targets[1].label).toMatch(/^shell-/);
    expect(onClose).toHaveBeenCalledTimes(2);

    press("remote.terminal.close");
    expect(onClose).toHaveBeenCalledTimes(3);
  });
});
