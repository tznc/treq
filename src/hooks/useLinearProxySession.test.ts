import { renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useLinearProxySession } from "./useLinearProxySession";

const { mockEnsureSync } = vi.hoisted(() => ({
  mockEnsureSync: vi.fn(),
}));

vi.mock("../lib/linear-proxy-auth", () => ({
  ensureLinearProxySessionSync: mockEnsureSync,
}));

describe("useLinearProxySession", () => {
  beforeEach(() => {
    mockEnsureSync.mockReset().mockResolvedValue(undefined);
  });

  it("starts the session sync when the integration is enabled", async () => {
    renderHook(() => useLinearProxySession(true));
    await vi.waitFor(() => expect(mockEnsureSync).toHaveBeenCalledOnce());
  });

  it("does nothing when the integration is disabled", () => {
    renderHook(() => useLinearProxySession(false));
    expect(mockEnsureSync).not.toHaveBeenCalled();
  });

  it("swallows sync errors", async () => {
    mockEnsureSync.mockRejectedValue(new Error("offline"));
    renderHook(() => useLinearProxySession(true));
    await vi.waitFor(() => expect(mockEnsureSync).toHaveBeenCalledOnce());
  });
});
