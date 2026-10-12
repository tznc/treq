import { afterEach, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn().mockResolvedValue(null),
  isTauri: () => true,
}));

vi.mock("@tauri-apps/api/event", async () => {
  const { listenOnTestBus } = await import("./test-event-bus");
  return {
    listen: vi.fn(listenOnTestBus),
    emit: vi.fn(),
  };
});

vi.mock("@tauri-apps/plugin-opener", () => ({
  openPath: vi.fn(),
  revealItemInDir: vi.fn(),
  openUrl: vi.fn(),
}));

afterEach(async () => {
  const { resetAllStores } = await import("../src/stores/resetStores");
  resetAllStores();
});
