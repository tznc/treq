// @include-parallel
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it } from "vitest";
import { Dashboard } from "../../src/components/Dashboard";
import { act } from "@testing-library/react";
import type { GpuiNativeEvent } from "../../src/lib/gpui-native";
import { type GpuiTerminalRoute } from "../../src/lib/gpui-screen";
import { emitOnTestBus } from "../test-event-bus";
import { useTreqSendStore } from "../../src/stores/treqSendStore";
import { findGpuiScreen, sendGpuiScreenAction } from "../gpui-screen";
import { render, screen, within } from "../test-utils";
import { createTestRepo, openRepo } from "../utils";

async function findTerminal(
  check?: (terminal: GpuiTerminalRoute) => void,
): Promise<GpuiTerminalRoute> {
  const workspace = await findGpuiScreen("workspace", (s) => {
    expect(s.terminal).not.toBeNull();
    check?.(s.terminal as GpuiTerminalRoute);
  });
  return workspace.terminal as GpuiTerminalRoute;
}

const terminalField = (field: string, value: unknown) =>
  sendGpuiScreenAction({
    kind: "field",
    field: `terminal.${field}`,
    value: value as Record<string, unknown>,
  });

describe("GPUI terminal pane", () => {
  let user: ReturnType<typeof userEvent.setup>;
  let repoPath: string;
  beforeEach(() => {
    window.__TREQ_REACT_TERMINAL__ = false;
    ({ repoPath } = createTestRepo(false));
    openRepo(repoPath);
    user = userEvent.setup();
  });

  async function submitTask(text: string) {
    const input = await screen.findByTestId("agent-task-input");
    await user.type(
      within(input).getByPlaceholderText("Describe a task..."),
      text,
    );
    await user.click(within(input).getByRole("button", { name: /^edit$/i }));
    return findTerminal((t) => {
      expect(t.pending.some((p) => p.pendingPrompt === text)).toBe(true);
    });
  }

  it("draws the pane in GPUI instead of React", async () => {
    render(<Dashboard />);
    await findTerminal();
    expect(screen.queryByTestId("workspace-terminal-pane")).toBeNull();
  });

  it("hands GPUI the new session's prompt and permission mode", async () => {
    render(<Dashboard />);
    await findTerminal();
    await user.keyboard("{Meta>}i{/Meta}");
    const dialog = (
      await screen.findByRole("heading", { name: "Start a new agent session" })
    ).closest('[data-testid="modal"]') as HTMLElement;
    await user.type(
      within(dialog).getByPlaceholderText("Describe a task..."),
      "Add a changelog entry",
    );
    await user.click(within(dialog).getByRole("button", { name: /^edit$/i }));

    const terminal = await findTerminal((t) => {
      expect(t.activeSessionId).not.toBeNull();
    });
    expect(
      terminal.pending.find((p) => p.sessionId === terminal.activeSessionId),
    ).toMatchObject({
      pendingPrompt: "Add a changelog entry",
      permissionMode: "acceptEdits",
    });
  });

  it("sends the pane's shortcuts while the webview has focus", async () => {
    render(<Dashboard />);
    await findTerminal();

    await user.keyboard("{Meta>}j{/Meta}");
    await user.keyboard("{Meta>}\\{/Meta}");
    await findTerminal((t) => {
      expect(t.commands.map((c) => c.kind)).toEqual([
        "toggleCollapse",
        "newShell",
      ]);
    });
  });

  it("follows the session GPUI activates and closes", async () => {
    render(<Dashboard />);
    await findTerminal();
    const { activeSessionId } = await submitTask("Work on the home repo");

    terminalField("activeSession", null);
    await findTerminal((t) => expect(t.activeSessionId).toBeNull());
    terminalField("activeSession", activeSessionId);
    await findTerminal((t) => expect(t.activeSessionId).toBe(activeSessionId));
    terminalField("closeSession", activeSessionId);
    await findTerminal((t) => expect(t.activeSessionId).toBeNull());
  }, 30000);

  it("hands GPUI the treq send attachments and drops the one it dismisses", async () => {
    render(<Dashboard />);
    await findTerminal((t) => expect(t.sendAssets).toEqual([]));
    useTreqSendStore.getState().ingestPayload({
      kind: "send",
      request_id: "req-1",
      repo: "/repo",
      pty_session_id: "session-1-1",
      media_type: "image",
      path: "/tmp/shot.png",
      title: "Shot",
    });
    await findTerminal((t) =>
      expect(t.sendAssets).toEqual([
        {
          id: "req-1",
          repo: "/repo",
          ptySessionId: "session-1-1",
          mediaType: "image",
          path: "/tmp/shot.png",
          title: "Shot",
        },
      ]),
    );
    terminalField("dismissAsset", "req-1");
    await findTerminal((t) => expect(t.sendAssets).toEqual([]));
    expect(useTreqSendStore.getState().assets).toEqual([]);
  }, 30000);

  it("asks React for a new agent when GPUI's pane does", async () => {
    render(<Dashboard />);
    await findTerminal();
    terminalField("newAgent", null);
    await findTerminal((t) => expect(t.activeSessionId).not.toBeNull());
  }, 30000);

  it("shows the session the backend started", async () => {
    render(<Dashboard />);
    await findTerminal();
    const event: GpuiNativeEvent = {
      kind: "agentSessionStarted",
      session: {
        sessionId: 42,
        sessionName: "AI Review",
        workspaceId: null,
        workspacePath: null,
        repoPath,
        pendingPrompt: "Review it",
        permissionMode: "acceptEdits",
      },
      paneSessionId: 42,
      select: true,
    };
    act(() => {
      emitOnTestBus("gpui-native-event", event);
    });
    const terminal = await findTerminal((t) =>
      expect(t.activeSessionId).toBe(42),
    );
    expect(terminal.pending.some((p) => p.sessionId === 42)).toBe(false);
  }, 30000);
});
