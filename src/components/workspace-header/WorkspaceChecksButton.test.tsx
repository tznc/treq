import { waitFor } from "@testing-library/react";
import { openUrl } from "@tauri-apps/plugin-opener";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { WorkspaceChecksButton } from "./WorkspaceChecksButton";
import * as api from "../../lib/api";
import type { PrCiStatus } from "../../lib/api-types";
import { render, screen, within } from "../../../test/test-utils";

afterEach(() => {
  vi.restoreAllMocks();
});

const TARGET = { repoPath: "/repo", workspaceId: 1, workspacePath: "/ws" };

// No Treq workflows, so only the PR's GitHub checks show.
function renderButton() {
  vi.spyOn(api, "listWorkflows").mockResolvedValue([]);
  vi.spyOn(api, "isRepoTrusted").mockResolvedValue(true);
  vi.spyOn(api, "getWorkspaceSetupStatus").mockResolvedValue({
    configured: false,
    status: "none",
  } as never);
  render(<WorkspaceChecksButton target={TARGET} prBranch="feat" />);
}

const baseStatus: PrCiStatus = {
  state: "success",
  total: 2,
  passed: 2,
  failed: 0,
  pending: 0,
  checks: [
    {
      name: "build",
      bucket: "pass",
      link: "https://x/1",
      duration_secs: 5 * 60 + 56,
    },
    { name: "lint", bucket: "pass", link: "https://x/2", duration_secs: 12 },
  ],
};

describe("WorkspaceChecksButton GitHub checks", () => {
  it("renders nothing while there is no CI status", async () => {
    const spy = vi.spyOn(api, "getCachedPrCiStatus").mockResolvedValue(null);
    vi.spyOn(api, "startPrStatusPolling").mockResolvedValue(undefined);
    vi.spyOn(api, "refreshPrBranchStatus").mockResolvedValue(undefined);

    renderButton();
    await waitFor(() => expect(spy).toHaveBeenCalled());
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
  });

  it("shows the passed/total ratio when CI succeeded", async () => {
    vi.spyOn(api, "getCachedPrCiStatus").mockResolvedValue(baseStatus);
    vi.spyOn(api, "startPrStatusPolling").mockResolvedValue(undefined);
    vi.spyOn(api, "refreshPrBranchStatus").mockResolvedValue(undefined);

    renderButton();

    expect(await screen.findByText("GH 2/2")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Checks: GitHub 2/2" }),
    ).toBeInTheDocument();
  });

  it("shows the failed state when CI failed", async () => {
    vi.spyOn(api, "getCachedPrCiStatus").mockResolvedValue({
      ...baseStatus,
      state: "failure",
      passed: 1,
      failed: 1,
      checks: [
        { name: "build", bucket: "pass", link: "https://x/1" },
        { name: "test", bucket: "fail", link: "https://x/2" },
      ],
    });
    vi.spyOn(api, "startPrStatusPolling").mockResolvedValue(undefined);
    vi.spyOn(api, "refreshPrBranchStatus").mockResolvedValue(undefined);

    renderButton();

    expect(
      await screen.findByTestId("workspace-checks-button"),
    ).toHaveTextContent("GH 1/2");
    expect(screen.getByTestId("run-status-failed")).toBeInTheDocument();
  });

  it("lists checks as failed, then pending, then success", async () => {
    const user = userEvent.setup();
    vi.spyOn(api, "getCachedPrCiStatus").mockResolvedValue({
      ...baseStatus,
      state: "failure",
      passed: 1,
      failed: 1,
      pending: 1,
      total: 3,
      checks: [
        {
          name: "build",
          bucket: "pass",
          link: "https://x/build",
          duration_secs: 12,
        },
        {
          name: "lint",
          bucket: "pending",
          link: "https://x/lint",
          duration_secs: 45,
        },
        {
          name: "test",
          bucket: "fail",
          link: "https://x/test",
          duration_secs: 5 * 60 + 56,
        },
      ],
    });
    vi.spyOn(api, "startPrStatusPolling").mockResolvedValue(undefined);
    vi.spyOn(api, "refreshPrBranchStatus").mockResolvedValue(undefined);

    renderButton();
    await user.click(
      await screen.findByRole("button", { name: "Checks: GitHub 1/3" }),
    );

    const section = within(
      await screen.findByRole("region", { name: "GitHub checks" }),
    );
    const checkButtons = section.getAllByRole("button", {
      name: /^(test|lint|build)/i,
    });
    expect(checkButtons.map((button) => button.textContent)).toEqual([
      "test5m 56s",
      "lint45s",
      "build12s",
    ]);
    expect(screen.queryByText("Failed")).not.toBeInTheDocument();
    expect(screen.queryByText("Success")).not.toBeInTheDocument();

    await user.click(section.getByRole("button", { name: /^test/i }));
    expect(openUrl).toHaveBeenCalledWith("https://x/test");
  });
});
