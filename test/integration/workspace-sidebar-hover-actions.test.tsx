// @include-parallel
import { beforeEach, describe, expect, it } from "vitest";
import { render, waitFor } from "../test-utils";
import { createTestRepo, openRepo } from "../utils";
import { createWorkspace } from "../../src/lib/api";
import { Dashboard } from "../../src/components/Dashboard";
import {
  findGpuiRow,
  gpuiSnapshot,
  openGpuiShell,
  stackGpui,
} from "../gpui-sidebar";
import { screen } from "@testing-library/react";

describe("workspace sidebar row actions", () => {
  beforeEach(async () => {
    const { repoPath } = createTestRepo(false);
    openRepo(repoPath);
    await createWorkspace(repoPath, "feat/alpha");
  });

  it("offers agent, shell and stack actions on a workspace row", async () => {
    render(<Dashboard />);
    await findGpuiRow("feat/alpha");

    await waitFor(() => {
      expect(gpuiSnapshot()?.agentUnavailable).toBeNull();
      expect(gpuiSnapshot()?.shellUnavailable).toBeNull();
    });

    await stackGpui("feat/alpha");
    expect(await screen.findByTestId("modal")).toBeTruthy();
    await openGpuiShell("feat/alpha");
    await waitFor(() =>
      expect(screen.getByTestId("show-workspace-header")).toHaveTextContent(
        "feat/alpha",
      ),
    );
  });
});
