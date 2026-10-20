// @include-parallel
import { waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it } from "vitest";
import { Dashboard } from "../../src/components/Dashboard";
import type { GpuiTerminalRoute } from "../../src/lib/gpui-screen";
import {
  findGpuiModal,
  findGpuiScreen,
  gpuiModal,
  sendGpuiScreenAction,
} from "../gpui-screen";
import { sendGpuiOverlayAction } from "../gpui-overlay";
import { render, screen } from "../test-utils";
import { createTestRepo, openRepo } from "../utils";

const pending = async (): Promise<GpuiTerminalRoute["pending"]> =>
  ((await findGpuiScreen("workspace")).terminal as GpuiTerminalRoute).pending;

describe("GPUI agent prompt, prompt history and stash dialogs", () => {
  let user: ReturnType<typeof userEvent.setup>;
  beforeEach(() => {
    window.__TREQ_REACT_TERMINAL__ = false;
    window.__TREQ_REACT_MODALS__ = false;
    openRepo(createTestRepo(false).repoPath);
    user = userEvent.setup();
  });

  it("draws the agent prompt dialog in GPUI instead of React", async () => {
    render(<Dashboard />);
    await findGpuiScreen("workspace");
    await user.keyboard("{Meta>}i{/Meta}");
    const modal = await findGpuiModal("agentPrompt");
    expect(modal.starting).toBe(false);
    expect(
      screen.queryByRole("heading", { name: "Start a new agent session" }),
    ).toBeNull();
  });

  it("opens the agent prompt for Cmd+I pressed in GPUI", async () => {
    render(<Dashboard />);
    await findGpuiScreen("workspace");
    sendGpuiOverlayAction({
      kind: "field",
      field: "overlay.shortcut",
      value: "i",
    });
    await findGpuiModal("agentPrompt");
  });

  it("closes the dialog when GPUI presses close", async () => {
    render(<Dashboard />);
    await findGpuiScreen("workspace");
    await user.keyboard("{Meta>}i{/Meta}");
    await findGpuiModal("agentPrompt");
    sendGpuiScreenAction({ kind: "press", id: "agentPrompt.close" });
    await waitFor(() => expect(gpuiModal()).toBeNull());
  });

  it("starts the session GPUI's dialog hands over", async () => {
    render(<Dashboard />);
    await findGpuiScreen("workspace");
    await user.keyboard("{Meta>}i{/Meta}");
    await findGpuiModal("agentPrompt");
    sendGpuiScreenAction({
      kind: "field",
      field: "agentPrompt.start",
      value: {
        prompt: "Add a changelog entry",
        agent: "claude",
        mode: "plan",
        workspaceId: null,
        saveAsRepoDefault: false,
        issue: false,
      },
    });
    await waitFor(async () => {
      expect(await pending()).toEqual(
        expect.arrayContaining([
          expect.objectContaining({
            pendingPrompt: "Add a changelog entry",
            permissionMode: "plan",
          }),
        ]),
      );
    });
    await waitFor(() => expect(gpuiModal()).toBeNull());
  });

  it("falls back to React's dialog while the flag asks for it", async () => {
    window.__TREQ_REACT_MODALS__ = true;
    render(<Dashboard />);
    await user.keyboard("{Meta>}i{/Meta}");
    expect(
      await screen.findByRole("heading", { name: "Start a new agent session" }),
    ).toBeTruthy();
    expect(gpuiModal() ?? null).toBeNull();
  });
});
