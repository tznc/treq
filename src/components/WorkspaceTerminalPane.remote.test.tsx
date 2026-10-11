import { beforeEach, describe, expect, it, vi } from "vitest";
import userEvent from "@testing-library/user-event";
import { act, render, screen } from "../../test/test-utils";
import {
  WorkspaceTerminalPane,
  type WorkspaceTerminalPaneHandle,
} from "./WorkspaceTerminalPane";
import type { RemoteTerminalTarget } from "./RemoteTerminalPanel";
import * as api from "../lib/api";
import * as apiExtra from "../lib/api-extra";
import type { SshEndpoint } from "../lib/api-types-remote";

vi.mock("../lib/api", async () => {
  const actual = await vi.importActual("../lib/api");
  return {
    ...actual,
    ptyCreateSession: vi.fn().mockResolvedValue(undefined),
    ptyClose: vi.fn().mockResolvedValue(undefined),
  };
});

vi.mock("../lib/api-extra", async () => {
  const actual = await vi.importActual("../lib/api-extra");
  return {
    ...actual,
    remotePtyListPersistentSessions: vi.fn().mockResolvedValue([]),
    remotePtyReattach: vi.fn().mockResolvedValue(undefined),
    remotePtyListen: vi.fn().mockResolvedValue(() => {}),
    remotePtyListenExit: vi.fn().mockResolvedValue(() => {}),
    remotePtyClose: vi.fn().mockResolvedValue(undefined),
    remotePtyResize: vi.fn().mockResolvedValue(undefined),
  };
});

const endpoint: SshEndpoint = {
  id: "ep1",
  instance_id: null,
  source: { type: "user_managed" },
  hostname: "vm.example.com",
  port: 22,
  username: "treq",
  host_keys: [],
  authentication: { type: "public_key", key_reference: "key1" },
};

const REPO = "/srv/project";

/** Captures the pane's imperative handle through a callback ref. */
const paneRef = () => {
  const ref = Object.assign(
    (handle: WorkspaceTerminalPaneHandle | null) => {
      ref.current = handle;
    },
    { current: null as WorkspaceTerminalPaneHandle | null },
  );
  return ref;
};

const shellTarget = (dir: string): RemoteTerminalTarget => ({
  endpoint,
  repositoryId: REPO,
  workspaceId: "root",
  remoteWorkingDirectory: dir,
  label: "shell-1",
  launch: { type: "shell" },
  reattach: false,
});

beforeEach(() => {
  vi.clearAllMocks();
});

describe("WorkspaceTerminalPane with a remote repository", () => {
  it("opens new shells on the remote host instead of a local PTY", async () => {
    const ref = paneRef();
    const resolveRemoteShell = vi.fn(shellTarget);
    render(
      <WorkspaceTerminalPane
        ref={ref}
        workingDirectory={REPO}
        resolveRemoteShell={resolveRemoteShell}
      />,
    );

    act(() => ref.current?.createShellSession(REPO));

    expect(resolveRemoteShell).toHaveBeenCalledWith(REPO);
    await vi.waitFor(() =>
      expect(apiExtra.remotePtyReattach).toHaveBeenCalledWith(
        expect.any(String),
        endpoint,
        REPO,
        "root",
        "shell-1",
        REPO,
        { type: "shell" },
        expect.any(Number),
        expect.any(Number),
      ),
    );
    expect(api.ptyCreateSession).not.toHaveBeenCalled();
  });

  it("does not open anything when the remote shell cannot be resolved", () => {
    const ref = paneRef();
    render(
      <WorkspaceTerminalPane
        ref={ref}
        workingDirectory={REPO}
        resolveRemoteShell={() => null}
      />,
    );

    act(() => ref.current?.createShellSession(REPO));

    expect(apiExtra.remotePtyReattach).not.toHaveBeenCalled();
    expect(api.ptyCreateSession).not.toHaveBeenCalled();
  });

  it("detaches from a remote agent session without killing it", async () => {
    const user = userEvent.setup();
    const ref = paneRef();
    render(<WorkspaceTerminalPane ref={ref} workingDirectory={REPO} />);

    act(() =>
      ref.current?.openRemoteSession({
        ...shellTarget(REPO),
        label: "claude-1",
        launch: { type: "agent", agent: "claude", args: [] },
      }),
    );
    await vi.waitFor(() =>
      expect(apiExtra.remotePtyReattach).toHaveBeenCalledTimes(1),
    );
    const [[localSessionId]] = vi.mocked(apiExtra.remotePtyReattach).mock.calls;

    await user.click(screen.getByRole("button", { name: "Detach" }));

    expect(apiExtra.remotePtyClose).toHaveBeenCalledWith(localSessionId);
    expect(api.ptyClose).not.toHaveBeenCalled();
    expect(screen.queryByRole("button", { name: "Detach" })).toBeNull();
  });
});

describe("WorkspaceTerminalPane wheel handling", () => {
  it("keeps horizontal swipes away from the terminal so the pane can scroll", () => {
    const ref = paneRef();
    render(
      <WorkspaceTerminalPane
        ref={ref}
        workingDirectory={REPO}
        resolveRemoteShell={shellTarget}
      />,
    );
    act(() => ref.current?.createShellSession(REPO));

    // Stands in for xterm's wheel listener, which cancels the event.
    const terminal = document.querySelector("[data-terminal-id]")!;
    const xtermWheel = vi.fn();
    terminal.addEventListener("wheel", xtermWheel);

    const wheel = (init: WheelEventInit) =>
      terminal.dispatchEvent(
        new WheelEvent("wheel", { bubbles: true, ...init }),
      );

    wheel({ deltaX: 40, deltaY: 2 });
    expect(xtermWheel).not.toHaveBeenCalled();

    wheel({ deltaX: 0, deltaY: 40 });
    expect(xtermWheel).toHaveBeenCalledTimes(1);
  });
});
