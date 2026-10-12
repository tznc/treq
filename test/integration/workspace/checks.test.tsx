// @include-parallel
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it } from "vitest";
import { Dashboard } from "../../../src/components/Dashboard";
import { createWorkspace, isRepoTrusted } from "../../../src/lib/api";
import { render, screen, waitFor, within } from "../../test-utils";
import {
  createTestRepo,
  findSidebarBranchElement,
  openRepo,
  writeRepoFile,
} from "../../utils";

const WORKFLOW = `
name: Logging CI
on:
  workflow_dispatch: {}
jobs:
  build:
    name: Build Job
    steps:
      - name: Emit output
        run: "echo hello-from-checks"
`;

let user: ReturnType<typeof userEvent.setup>;

async function openWorkspace(branch: string) {
  const { repoPath } = createTestRepo(false);
  await writeRepoFile(repoPath, ".treq/workflows/ci.yaml", WORKFLOW);
  openRepo(repoPath);
  await createWorkspace(repoPath, branch);
  render(<Dashboard />);
  await user.click(await findSidebarBranchElement(branch));
  return repoPath;
}

describe("Workspace checks", () => {
  beforeEach(() => {
    user = userEvent.setup();
  });

  it("trusts the repo and runs a workflow from the Logs tab", async () => {
    const repoPath = await openWorkspace("feat/checks-logs");
    await user.click(await screen.findByRole("tab", { name: /^Logs/ }));

    const checks = await screen.findByTestId(
      "workspace-checks",
      {},
      { timeout: 20_000 },
    );
    await user.click(
      await within(checks).findByRole("button", { name: "Trust Repository" }),
    );
    await waitFor(async () => expect(await isRepoTrusted(repoPath)).toBe(true));

    const run = await within(checks).findByRole("button", {
      name: "Run Logging CI",
    });
    await waitFor(() =>
      expect((run as HTMLButtonElement).disabled).toBe(false),
    );
    await user.click(run);

    await within(checks).findByRole(
      "button",
      { name: "Re-run Logging CI" },
      { timeout: 20_000 },
    );
    expect(
      within(checks).getAllByTestId("run-status-passed").length,
    ).toBeGreaterThanOrEqual(2);
    await screen.findByText("hello-from-checks", {}, { timeout: 20_000 });
  });

  it("shows a header summary and runs all checks from it", async () => {
    await openWorkspace("feat/checks-header");

    const indicator = await screen.findByTestId(
      "workspace-checks-indicator",
      {},
      { timeout: 20_000 },
    );
    expect(indicator.textContent).toContain("Checks not run");
    const runAll = within(indicator).getByRole("button", {
      name: "Run all checks",
    });
    expect((runAll as HTMLButtonElement).disabled).toBe(true);

    await user.click(within(indicator).getByRole("button", { name: /Checks/ }));
    const checks = await screen.findByTestId("workspace-checks");
    await user.click(
      await within(checks).findByRole("button", { name: "Trust Repository" }),
    );

    await waitFor(() =>
      expect((runAll as HTMLButtonElement).disabled).toBe(false),
    );
    await user.click(runAll);
    await waitFor(() => expect(indicator.dataset.state).toBe("passed"), {
      timeout: 20_000,
    });
    expect(indicator.textContent).toContain("Checks 1/1");
  });
});
