// @include-parallel
import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { render, waitFor } from "../test-utils";
import { createTestRepo, openRepo } from "../utils";
import { createWorkspace } from "../../src/lib/api";
import { Dashboard } from "../../src/components/Dashboard";
import {
  clickGpuiRow,
  findGpuiRow,
  findGpuiToast,
  gpuiSnapshot,
  sendGpuiAction,
  waitForNoGpuiRow,
} from "../gpui-sidebar";

describe("bulk archive workspaces", () => {
  let repoPath: string;

  beforeEach(async () => {
    ({ repoPath } = createTestRepo(false));
    openRepo(repoPath);
    await createWorkspace(repoPath, "feat/alpha");
    await createWorkspace(repoPath, "feat/beta");
  });

  it("greys out selected workspaces with spinners while bulk archive runs", async () => {
    const originalInvoke = vi.mocked(invoke).getMockImplementation();
    expect(originalInvoke).toBeTruthy();
    let releaseArchives: (() => void) | undefined;
    const archiveGate = new Promise<void>((resolve) => {
      releaseArchives = resolve;
    });
    vi.mocked(invoke).mockImplementation(async (cmd, args) => {
      if (cmd === "archive_workspace") {
        await archiveGate;
      }
      return originalInvoke!(cmd, args);
    });

    try {
      render(<Dashboard />);

      await findGpuiRow("feat/alpha");
      await findGpuiRow("feat/beta");

      await clickGpuiRow("feat/alpha", { toggle: true });
      await clickGpuiRow("feat/beta", { toggle: true });

      await waitFor(() => expect(gpuiSnapshot()?.bulkArchive).toBe(2));
      sendGpuiAction({ kind: "bulkArchive" });

      await waitFor(async () => {
        expect((await findGpuiRow("feat/alpha")).archiving).toBe(true);
        expect((await findGpuiRow("feat/beta")).archiving).toBe(true);
      });
      expect(gpuiSnapshot()?.bulkArchive).toBeNull();

      releaseArchives!();

      await findGpuiToast(/2 workspaces archived/i);
      await waitForNoGpuiRow("feat/alpha");
      await waitForNoGpuiRow("feat/beta");
    } finally {
      vi.mocked(invoke).mockImplementation(originalInvoke!);
    }
  });
});
