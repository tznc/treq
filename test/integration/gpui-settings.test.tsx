// @include-parallel
import * as React from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { Dashboard } from "../../src/components/Dashboard";
import {
  findGpuiScreen,
  gpuiScreen,
  sendGpuiScreenAction,
} from "../gpui-screen";
import { findGpuiToast, gpuiSnapshot, sendGpuiAction } from "../gpui-sidebar";
import { render, screen } from "../test-utils";
import { createTestRepo, openRepo } from "../utils";

async function openGpuiSettings() {
  await waitFor(() => expect(gpuiSnapshot()).toBeTruthy());
  sendGpuiAction({ kind: "openSettings" });
}

describe("GPUI settings screen", () => {
  beforeEach(() => {
    const { repoPath } = createTestRepo(false);
    openRepo(repoPath);
  });

  it("draws the repository tab with the fields React loaded", async () => {
    render(<Dashboard />);
    await openGpuiSettings();

    const settings = await findGpuiScreen("settings", (s) => {
      expect(s.repository?.loading).toBe(false);
    });
    expect(settings.tab).toBe("repository");
    expect(settings.tabs).toContain("preview");
    expect(settings.save).toEqual({ label: "Save Settings", disabled: false });
    const pattern = (await screen.findByLabelText(
      /branch name pattern/i,
    )) as HTMLInputElement;
    expect(settings.repository?.fields.branchNamePattern).toBe(pattern.value);
    expect(settings.repository?.modelOptions[0]).toEqual({
      value: "",
      label: "Use Application Default",
    });
  });

  it("switches tabs and saves application settings through React", async () => {
    render(<Dashboard />);
    await openGpuiSettings();
    await findGpuiScreen("settings");

    sendGpuiScreenAction({
      kind: "field",
      field: "settings.tab",
      value: "application",
    });
    const app = await findGpuiScreen("settings", (s) => {
      expect(s.tab).toBe("application");
    });
    expect(app.application?.fontSizeOptions.map((o) => o.label)).toContain(
      "Large",
    );
    const large = app.application?.fontSizeOptions.find(
      (o) => o.label === "Large",
    );

    sendGpuiScreenAction({
      kind: "field",
      field: "app.fontSize",
      value: large?.value ?? "",
    });
    await findGpuiScreen("settings", (s) => {
      expect(s.application?.fontSize).toBe(large?.value);
    });
    sendGpuiScreenAction({ kind: "press", id: "settings.save" });
    await findGpuiToast("Settings Saved");
  });

  it("opens install links from the command-line tools list", async () => {
    render(<Dashboard />);
    await openGpuiSettings();
    sendGpuiScreenAction({
      kind: "field",
      field: "settings.tab",
      value: "application",
    });
    await findGpuiScreen("settings", (s) => {
      expect(s.application?.checklist.groups.length).toBeGreaterThan(0);
    });

    sendGpuiScreenAction({ kind: "openUrl", url: "https://cli.github.com/" });
    expect(vi.mocked(openUrl)).toHaveBeenCalledWith("https://cli.github.com/");
  });

  it("leaves tabs GPUI does not draw to the webview", async () => {
    render(<Dashboard />);
    await openGpuiSettings();
    await findGpuiScreen("settings");

    sendGpuiScreenAction({
      kind: "field",
      field: "settings.tab",
      value: "account",
    });
    await waitFor(() => expect(gpuiScreen()).toBeNull());
    expect(
      await screen.findByRole("tab", { name: /account/i }),
    ).toHaveAttribute("data-state", "active");
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
