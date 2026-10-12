// @include-parallel
import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { render, screen, waitFor } from "../test-utils";
import userEvent from "@testing-library/user-event";
import { createTestRepo, openRepo } from "../utils";
import { createWorkspace } from "../../src/lib/api";
import { Dashboard } from "../../src/components/Dashboard";

describe("bulk archive workspaces", () => {
  let repoPath: string;
  let user: ReturnType<typeof userEvent.setup>;

  beforeEach(async () => {
    ({ repoPath } = createTestRepo(false));
    openRepo(repoPath);
    await createWorkspace(repoPath, "feat/alpha");
    await createWorkspace(repoPath, "feat/beta");
    user = userEvent.setup();
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

      const alpha = await screen.findByTestId(
        "workspace-sidebar-item-feat/alpha",
      );
      const beta = await screen.findByTestId(
        "workspace-sidebar-item-feat/beta",
      );

      await user.keyboard("{Meta>}");
      await user.click(alpha);
      await user.click(beta);
      await user.keyboard("{/Meta}");

      await user.click(await screen.findByText(/archive 2 workspaces/i));

      await waitFor(() => {
        expect(alpha).toHaveAttribute("aria-busy", "true");
        expect(beta).toHaveAttribute("aria-busy", "true");
      });
      expect(
        screen.queryByText(/archive 2 workspaces/i),
      ).not.toBeInTheDocument();
      expect(alpha).toHaveAttribute("aria-disabled", "true");
      expect(beta).toHaveAttribute("aria-disabled", "true");
      expect(
        alpha.querySelector('[data-testid="workspace-archive-spinner"]'),
      ).toBeTruthy();
      expect(
        beta.querySelector('[data-testid="workspace-archive-spinner"]'),
      ).toBeTruthy();

      releaseArchives!();

      await screen.findByText(/2 workspaces archived/i);
      await waitFor(() => {
        expect(
          screen.queryByTestId("workspace-sidebar-item-feat/alpha"),
        ).toBeNull();
        expect(
          screen.queryByTestId("workspace-sidebar-item-feat/beta"),
        ).toBeNull();
      });
    } finally {
      vi.mocked(invoke).mockImplementation(originalInvoke!);
    }
  });
});
