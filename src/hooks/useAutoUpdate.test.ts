import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const addToast = vi.fn();
const check = vi.fn();
const relaunch = vi.fn();
const ask = vi.fn();

vi.mock("../components/ui/toast", () => ({ useToast: () => ({ addToast }) }));
vi.mock("@tauri-apps/plugin-updater", () => ({ check: () => check() }));
vi.mock("@tauri-apps/plugin-process", () => ({ relaunch: () => relaunch() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({
  ask: (...a: unknown[]) => ask(...a),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async () => () => {}),
}));

import { useAutoUpdate } from "./useAutoUpdate";

const flush = () => new Promise((r) => setTimeout(r, 0));

describe("useAutoUpdate", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("reports up to date when the updater finds nothing", async () => {
    check.mockResolvedValue(null);
    const { result } = renderHook(() =>
      useAutoUpdate({ autoCheck: false, listenMenu: false }),
    );
    await act(() => result.current.checkForUpdate());
    expect(addToast).toHaveBeenCalledWith(
      expect.objectContaining({ title: "You're up to date" }),
    );
  });

  it("downloads, installs and relaunches on confirm", async () => {
    const downloadAndInstall = vi.fn().mockResolvedValue(undefined);
    check.mockResolvedValue({
      version: "0.4.0",
      currentVersion: "0.3.0",
      downloadAndInstall,
    });
    ask.mockResolvedValue(true);
    const { result } = renderHook(() =>
      useAutoUpdate({ autoCheck: false, listenMenu: false }),
    );
    await act(() => result.current.checkForUpdate());
    const [[prompt]] = addToast.mock.calls;
    expect(prompt.title).toBe("Update available: v0.4.0");
    await act(async () => {
      prompt.action.onClick();
      await flush();
    });
    expect(downloadAndInstall).toHaveBeenCalledOnce();
    expect(relaunch).toHaveBeenCalledOnce();
  });

  it("surfaces check errors on a manual check", async () => {
    check.mockRejectedValue(new Error("signature mismatch"));
    const { result } = renderHook(() =>
      useAutoUpdate({ autoCheck: false, listenMenu: false }),
    );
    await act(() => result.current.checkForUpdate());
    expect(addToast).toHaveBeenCalledWith(
      expect.objectContaining({
        title: "Update check failed",
        description: "signature mismatch",
      }),
    );
  });
});
