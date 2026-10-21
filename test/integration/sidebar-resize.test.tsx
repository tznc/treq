// @include-parallel
import * as React from "react";
import { beforeEach, describe, expect, it } from "vitest";
import { render, waitFor } from "../test-utils";
import { createTestRepo, openRepo } from "../utils";
import { createWorkspace } from "../../src/lib/api";
import { Dashboard } from "../../src/components/Dashboard";
import {
  DEFAULT_SIDEBAR_WIDTH,
  useSidebarWidthStore,
} from "../../src/stores/sidebarWidthStore";
import { findGpuiRow, gpuiSnapshot, sendGpuiAction } from "../gpui-sidebar";

describe("Dashboard - workspace sidebar resize", () => {
  let repoPath: string;

  beforeEach(async () => {
    ({ repoPath } = createTestRepo(false));
    openRepo(repoPath);
    await createWorkspace(repoPath, "feat/alpha");
  });

  it("resizes the workspace sidebar by dragging the edge", async () => {
    render(<Dashboard />);
    await findGpuiRow("feat/alpha");
    expect(gpuiSnapshot()?.width).toBe(DEFAULT_SIDEBAR_WIDTH);

    sendGpuiAction({
      kind: "resizeSidebar",
      width: DEFAULT_SIDEBAR_WIDTH + 80,
    });

    await waitFor(() =>
      expect(useSidebarWidthStore.getState().width).toBe(
        DEFAULT_SIDEBAR_WIDTH + 80,
      ),
    );
    await waitFor(() =>
      expect(gpuiSnapshot()?.width).toBe(DEFAULT_SIDEBAR_WIDTH + 80),
    );
  });
});
