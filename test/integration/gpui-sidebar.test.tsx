// @include-parallel
import * as React from "react";
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within } from "../test-utils";
import { createTestRepo, openRepo } from "../utils";
import { createWorkspace } from "../../src/lib/api";
import { Dashboard } from "../../src/components/Dashboard";
import {
  clickGpuiHome,
  clickGpuiRow,
  findGpuiRow,
  gpuiRepo,
  gpuiRows,
  gpuiSnapshot,
  gpuiTitles,
  sendGpuiAction,
  stackGpui,
} from "../gpui-sidebar";

describe("Dashboard - GPUI sidebar host", () => {
  beforeEach(async () => {
    const { repoPath } = createTestRepo(false);
    openRepo(repoPath);
    await createWorkspace(repoPath, "feat/alpha");
  });

  it("selects a workspace from a GPUI click", async () => {
    render(<Dashboard />);

    await waitFor(() => expect(gpuiTitles()).toEqual(["feat/alpha"]));
    expect(gpuiRepo()?.homeSelected).toBe(true);

    await clickGpuiRow("feat/alpha");

    await waitFor(() => {
      expect(gpuiRepo()?.homeSelected).toBe(false);
      expect(gpuiRows()[0].selected).toBe(true);
    });

    await clickGpuiHome();
    await waitFor(() => expect(gpuiRepo()?.homeSelected).toBe(true));
  });
});

describe("Dashboard - GPUI sidebar actions", () => {
  beforeEach(async () => {
    const { repoPath } = createTestRepo(false);
    openRepo(repoPath);
    await createWorkspace(repoPath, "feat/alpha");
  });

  it("pushes the home branch and local capabilities", async () => {
    render(<Dashboard />);
    await waitFor(() => expect(gpuiRepo()?.homeRef).toBeTruthy());
    expect(gpuiRepo()).toMatchObject({ active: true });
    expect(gpuiSnapshot()).toMatchObject({
      agentUnavailable: null,
      shellUnavailable: null,
      bulkArchive: null,
    });
  });

  it("opens the stack dialog from the home row", async () => {
    render(<Dashboard />);
    await waitFor(() => expect(gpuiRepo()?.homeRef).toBeTruthy());
    await stackGpui(null);
    const dialog = await screen.findByTestId("modal");
    expect(within(dialog).getByLabelText("Branch Name")).toBeTruthy();
  });

  it("opens the stack dialog from a workspace row", async () => {
    render(<Dashboard />);
    await findGpuiRow("feat/alpha");
    await stackGpui("feat/alpha");
    const dialog = await screen.findByTestId("modal");
    expect(within(dialog).getByLabelText("Branch Name")).toBeTruthy();
  });

  it("ignores actions for workspaces it does not know", async () => {
    render(<Dashboard />);
    await findGpuiRow("feat/alpha");
    sendGpuiAction({ kind: "stack", id: 999_999 });
    sendGpuiAction({
      kind: "selectWorkspace",
      id: 999_999,
      shift: false,
      toggle: false,
    });
    expect(screen.queryByTestId("modal")).toBeNull();
    expect(gpuiRepo()?.homeSelected).toBe(true);
  });
});

describe("Dashboard - without the GPUI shell", () => {
  it("pushes no snapshot and renders no sidebar", async () => {
    delete window.__TREQ_GPUI_SHELL__;
    const { repoPath } = createTestRepo(false);
    openRepo(repoPath);
    vi.mocked(invoke).mockClear();
    render(<Dashboard />);
    expect(await screen.findByTestId("show-workspace-header")).toBeTruthy();
    expect(screen.queryByTestId("workspace-sidebar")).toBeNull();
    expect(
      vi
        .mocked(invoke)
        .mock.calls.some(([cmd]) => cmd === "gpui_sidebar_update"),
    ).toBe(false);
  });
});
