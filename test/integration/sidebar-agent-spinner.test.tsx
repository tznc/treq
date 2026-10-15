// @include-parallel
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { Dashboard } from "../../src/components/Dashboard";
import { createWorkspace } from "../../src/lib/api";
import { installFakeAgents } from "../fake-agent";
import { clickGpuiRow, findGpuiRow } from "../gpui-sidebar";
import { render, screen, waitFor } from "../test-utils";
import { createTestRepo, openRepo } from "../utils";

describe("sidebar agent spinner", () => {
  let restoreAgents: () => void;
  let user: ReturnType<typeof userEvent.setup>;

  beforeEach(() => {
    restoreAgents = installFakeAgents();
    user = userEvent.setup();
  });

  afterEach(() => {
    restoreAgents();
  });

  it("spins on a clean workspace while its agent works, and stops when it goes idle", async () => {
    const { repoPath } = createTestRepo(false);
    openRepo(repoPath);
    await createWorkspace(repoPath, "feat/clean-agent");
    render(<Dashboard />);

    await clickGpuiRow("feat/clean-agent");
    await screen.findByTestId("workspace-terminal-pane");
    expect((await findGpuiRow("feat/clean-agent")).status).toBeNull();

    await user.keyboard("{Meta>}]{/Meta}");

    await waitFor(
      async () =>
        expect((await findGpuiRow("feat/clean-agent")).status).toEqual(
          expect.objectContaining({ spin: true }),
        ),
      { timeout: 15000 },
    );

    await waitFor(
      async () =>
        expect((await findGpuiRow("feat/clean-agent")).status).toBeNull(),
      { timeout: 15000 },
    );
  }, 60000);
});
