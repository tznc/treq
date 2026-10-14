// @include-parallel
import { beforeEach, describe, expect, it, vi } from "vitest";
import { listen } from "@tauri-apps/api/event";
import * as api from "../../../src/lib/api";
import { createWorkspace, getWorkspaces } from "../../../src/lib/api";
import { render, screen, waitFor } from "../../test-utils";
import { Dashboard } from "../../../src/components/Dashboard";
import {
  createTestRepo,
  openRepo,
} from "../../utils";
import { TREQ_SEND_EVENT } from "../../../src/lib/treqSend";
import { useFeaturePreviewStore } from "../../../src/stores/featurePreviewStore";
import { useTreqSendStore } from "../../../src/stores/treqSendStore";
import { clickGpuiRow } from "../../gpui-sidebar";

describe("treq send --browser integration", () => {

  beforeEach(() => {
    useFeaturePreviewStore.setState({
      flags: {
        ...useFeaturePreviewStore.getState().flags,
        browser: true,
      },
    });
    useTreqSendStore.setState({ assets: [] });
  });

  it("opens the Browser view and navigates when a browser send event arrives", async () => {
    const { repoPath } = createTestRepo(false);
    openRepo(repoPath);
    const workspaceId = await createWorkspace(repoPath, "feat/browser-send");
    const workspace = (await getWorkspaces(repoPath)).find(
      (item) => item.id === workspaceId,
    );
    if (!workspace) throw new Error("workspace not found");

    vi.spyOn(api, "openBrowserWebview").mockResolvedValue(undefined);
    vi.spyOn(api, "navigateBrowserWebview").mockResolvedValue(undefined);
    vi.spyOn(api, "closeBrowserWebview").mockResolvedValue(undefined);
    vi.spyOn(api, "syncBrowserWebviewBounds").mockResolvedValue(undefined);
    vi.spyOn(api, "listenBrowserUrlChanged").mockImplementation(() =>
      Promise.resolve(() => {}),
    );

    render(<Dashboard />);
    await clickGpuiRow(workspace.branch_name);

    const listenMock = vi.mocked(listen);
    const call = listenMock.mock.calls.find(
      ([eventName]) => eventName === TREQ_SEND_EVENT,
    );
    expect(call).toBeTruthy();
    const handler = call![1] as (event: { payload: unknown }) => void;

    handler({
      payload: {
        kind: "send",
        request_id: "send-browser-1",
        repo: repoPath,
        media_type: "browser",
        path: "http://localhost:5173/preview",
        title: "http://localhost:5173/preview",
      },
    });

    await screen.findByRole("tab", { name: /^Changes/, selected: true });
    await waitFor(() => {
      expect(api.openBrowserWebview).toHaveBeenCalledWith(
        "http://localhost:5173/preview",
        expect.any(Number),
        expect.any(Number),
        expect.any(Number),
        expect.any(Number),
      );
    });
  });

  it("ignores browser send events when the Browser feature is disabled", async () => {
    const { repoPath } = createTestRepo(false);
    openRepo(repoPath);
    const workspaceId = await createWorkspace(
      repoPath,
      "feat/browser-disabled",
    );
    const workspace = (await getWorkspaces(repoPath)).find(
      (item) => item.id === workspaceId,
    );
    if (!workspace) throw new Error("workspace not found");

    vi.spyOn(api, "openBrowserWebview").mockResolvedValue(undefined);
    vi.mocked(api.openBrowserWebview).mockClear();
    render(<Dashboard />);
    await clickGpuiRow(workspace.branch_name);
    const listenMock = vi.mocked(listen);
    const call = listenMock.mock.calls.find(
      ([eventName]) => eventName === TREQ_SEND_EVENT,
    );
    expect(call).toBeTruthy();
    useFeaturePreviewStore.setState({
      flags: {
        ...useFeaturePreviewStore.getState().flags,
        browser: false,
      },
    });
    await waitFor(() => {
      expect(useFeaturePreviewStore.getState().flags.browser).toBe(false);
    });

    (call![1] as (event: { payload: unknown }) => void)({
      payload: {
        kind: "send",
        request_id: "send-browser-disabled",
        repo: repoPath,
        media_type: "browser",
        path: "http://localhost:5173/preview",
        title: "http://localhost:5173/preview",
      },
    });

    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(api.openBrowserWebview).not.toHaveBeenCalled();
  });
});
