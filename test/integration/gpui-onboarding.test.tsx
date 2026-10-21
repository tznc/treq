// @include-parallel
import * as React from "react";
import { act } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { Dashboard } from "../../src/components/Dashboard";
import type { GpuiNativeEvent } from "../../src/lib/gpui-native";
import {
  findGpuiScreen,
  gpuiScreen,
  sendGpuiScreenAction,
} from "../gpui-screen";
import { findGpuiToast } from "../gpui-sidebar";
import { emitOnTestBus } from "../test-event-bus";
import { render, screen } from "../test-utils";
import { createTestRepo } from "../utils";

function sendNativeEvent(event: GpuiNativeEvent) {
  act(() => {
    emitOnTestBus("gpui-native-event", event);
  });
}

describe("GPUI onboarding screen", () => {
  beforeEach(() => {
    window.history.replaceState({}, "", "/");
  });

  it("asks GPUI for the screen without data of its own", async () => {
    render(<Dashboard />);
    const snapshot = await findGpuiScreen("onboarding");
    expect(Object.keys(snapshot).sort()).toEqual(["dark", "kind"]);
  });

  it("switches to a repository GPUI opened", async () => {
    const { repoPath } = createTestRepo(false);
    await invoke("init_repo", { repoPath });
    render(<Dashboard />);
    await findGpuiScreen("onboarding");

    sendNativeEvent({ kind: "repoOpened", path: repoPath });
    sendNativeEvent({
      kind: "toast",
      title: "Repository Opened",
      description: "Now viewing repo",
      tone: "success",
    });
    await vi.waitFor(() => expect(gpuiScreen()?.kind).not.toBe("onboarding"));
    await findGpuiToast("Repository Opened");
  });

  it("brings the webview back while a React dialog is open", async () => {
    render(<Dashboard />);
    await findGpuiScreen("onboarding");

    sendGpuiScreenAction({ kind: "openRemote" });
    await screen.findByRole("dialog");
    await vi.waitFor(() => expect(gpuiScreen()).toBeNull());
  });
});
