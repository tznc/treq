// @include-parallel
import { beforeEach, describe, expect, it } from "vitest";
import { render, screen } from "../test-utils";
import { waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { createTestRepo, openRepo } from "../utils";
import { createWorkspace } from "../../src/lib/api";
import { Dashboard } from "../../src/components/Dashboard";
import {
  clickGpuiRow,
  findGpuiRow,
  gpuiRepo,
  pickGpuiHomeMenuItem,
} from "../gpui-sidebar";

describe("home row context menu does not lock sidebar clicks after branch switch", () => {
  let repoPath: string;
  let user: ReturnType<typeof userEvent.setup>;

  beforeEach(async () => {
    ({ repoPath } = createTestRepo(false));
    openRepo(repoPath);
    await createWorkspace(repoPath, "feat/alpha");
    await createWorkspace(repoPath, "feat/beta");
    user = userEvent.setup();
  });

  it("switches the home branch via Switch Branch... and still allows clicking a workspace row afterward", async () => {
    render(<Dashboard />);

    await findGpuiRow("feat/alpha");
    await waitFor(() => expect(gpuiRepo()?.homeRef).toBeTruthy());
    expect(gpuiRepo()?.homeRef).not.toBe("feat/beta");

    await pickGpuiHomeMenuItem("Switch Branch...");

    const modal = await screen.findByTestId("modal");
    const targetBranchOption = await within(modal).findByText("feat/beta");
    await user.click(targetBranchOption);

    await waitFor(() => expect(gpuiRepo()?.homeRef).toBe("feat/beta"));
    expect(screen.queryByTestId("modal")).not.toBeInTheDocument();

    expect(document.body.style.pointerEvents).not.toBe("none");

    await clickGpuiRow("feat/alpha");

    await waitFor(async () =>
      expect((await findGpuiRow("feat/alpha")).selected).toBe(true),
    );
  });
});
