// @include-parallel
import * as React from "react";
import { act, waitFor } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it } from "vitest";
import { Dashboard } from "../../src/components/Dashboard";
import type { GpuiNativeEvent } from "../../src/lib/gpui-native";
import {
  findGpuiScreen,
  gpuiScreen,
  sendGpuiScreenAction,
} from "../gpui-screen";
import { gpuiSnapshot, sendGpuiAction } from "../gpui-sidebar";
import { emitOnTestBus } from "../test-event-bus";
import { render, screen } from "../test-utils";
import { createTestRepo, openRepo } from "../utils";

async function openGpuiSettings() {
  await waitFor(() => expect(gpuiSnapshot()).toBeTruthy());
  sendGpuiAction({ kind: "openSettings" });
}

function sendNativeEvent(event: GpuiNativeEvent) {
  act(() => {
    emitOnTestBus("gpui-native-event", event);
  });
}

function showTab(tab: string) {
  sendGpuiScreenAction({ kind: "field", field: "settings.tab", value: tab });
}

describe("GPUI settings screen", () => {
  let repoPath: string;

  beforeEach(() => {
    ({ repoPath } = createTestRepo(false));
    openRepo(repoPath);
  });

  it("asks GPUI for the tab and repository only", async () => {
    render(<Dashboard />);
    await openGpuiSettings();

    const settings = await findGpuiScreen("settings");
    expect(Object.keys(settings).sort()).toEqual([
      "currentBranch",
      "dark",
      "kind",
      "repoPath",
      "tab",
    ]);
    expect(settings.tab).toBe("repository");
    expect(settings.repoPath).toBe(repoPath);
  });

  it("switches tabs through React", async () => {
    render(<Dashboard />);
    await openGpuiSettings();
    await findGpuiScreen("settings");

    showTab("preview");
    await findGpuiScreen("settings", (s) => expect(s.tab).toBe("preview"));
    showTab("skills");
    await findGpuiScreen("settings", (s) => expect(s.tab).toBe("skills"));
    showTab("account");
    await waitFor(() => expect(gpuiScreen()).toBeNull());
    expect(
      await screen.findByRole("tab", { name: /account/i }),
    ).toHaveAttribute("data-state", "active");
  });

  it("shows application settings GPUI saved", async () => {
    render(<Dashboard />);
    await openGpuiSettings();
    await findGpuiScreen("settings");

    await invoke("set_setting", { key: "default_agent", value: "codex" });
    sendNativeEvent({ kind: "settingsChanged" });
    showTab("application");
    await findGpuiScreen("settings", (s) => expect(s.tab).toBe("application"));
    await waitFor(() =>
      expect(
        (screen.getByLabelText("Default Agent") as HTMLSelectElement).value,
      ).toBe("codex"),
    );
  });

  it("shows repository settings GPUI saved", async () => {
    render(<Dashboard />);
    await openGpuiSettings();
    await findGpuiScreen("settings");
    await screen.findByLabelText(/branch name pattern/i);

    await invoke("set_repo_setting", {
      repoPath,
      key: "branch_name_pattern",
      value: "team/{name}",
    });
    sendNativeEvent({ kind: "repoSettingsChanged", repoPath });
    await waitFor(() =>
      expect(
        (screen.getByLabelText(/branch name pattern/i) as HTMLInputElement)
          .value,
      ).toBe("team/{name}"),
    );
  });

  it("closes back to the workspace view", async () => {
    render(<Dashboard />);
    await openGpuiSettings();
    await findGpuiScreen("settings");

    sendGpuiScreenAction({ kind: "press", id: "settings.close" });
    await findGpuiScreen("workspace");
    expect(screen.queryByRole("heading", { name: "Settings" })).toBeNull();
  });
});
