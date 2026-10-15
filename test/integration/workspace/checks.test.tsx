// @include-parallel
import { clickGpuiRow } from "../../gpui-sidebar";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it } from "vitest";
import { Dashboard } from "../../../src/components/Dashboard";
import { createWorkspace, isRepoTrusted } from "../../../src/lib/api";
import { render, screen, waitFor, within } from "../../test-utils";
import { createTestRepo, openRepo, writeRepoFile } from "../../utils";

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

const DOCS_WORKFLOW = `
name: Docs
on:
  workflow_dispatch: {}
jobs:
  docs:
    name: Docs Job
    steps:
      - name: Emit output
        run: "echo hello-from-docs"
`;

let user: ReturnType<typeof userEvent.setup>;

async function openWorkspace(branch: string, extraWorkflows = false) {
  const { repoPath } = createTestRepo(false);
  await writeRepoFile(repoPath, ".treq/workflows/ci.yaml", WORKFLOW);
  if (extraWorkflows) {
    await writeRepoFile(repoPath, ".treq/workflows/docs.yaml", DOCS_WORKFLOW);
  }
  openRepo(repoPath);
  await createWorkspace(repoPath, branch);
  render(<Dashboard />);
  await clickGpuiRow(branch);
  return repoPath;
}

async function openChecksPopover() {
  await user.click(
    await screen.findByTestId(
      "workspace-checks-button",
      {},
      { timeout: 20_000 },
    ),
  );
  return screen.findByTestId("checks-popover");
}

async function trustAndRunAll(repoPath: string) {
  const popover = await openChecksPopover();
  await user.click(
    await within(popover).findByRole("button", { name: "Trust Repository" }),
  );
  await waitFor(async () => expect(await isRepoTrusted(repoPath)).toBe(true));
  const runAll = within(popover).getByRole("button", {
    name: "Run all checks",
  });
  await waitFor(() =>
    expect((runAll as HTMLButtonElement).disabled).toBe(false),
  );
  await user.click(runAll);
  return popover;
}

describe("Workspace checks", () => {
  beforeEach(() => {
    user = userEvent.setup();
  });

  it("trusts the repo and runs all checks from the header popover", async () => {
    const repoPath = await openWorkspace("feat/checks-header", true);

    const trigger = await screen.findByTestId(
      "workspace-checks-button",
      {},
      { timeout: 20_000 },
    );
    expect(trigger.textContent).toContain("not run");
    const popover = await trustAndRunAll(repoPath);

    await waitFor(() => expect(trigger.dataset.checksState).toBe("passed"), {
      timeout: 20_000,
    });
    expect(trigger.textContent).toContain("2/2");
    const rows = within(popover).getAllByTestId("checks-popover-workflow");
    expect(rows).toHaveLength(2);
    for (const row of rows) expect(row.textContent).toMatch(/run #\d+ passed/);

    await user.click(
      within(popover).getByRole("button", { name: "View logs" }),
    );
    await screen.findByRole("tab", { name: /^Logs/, selected: true });
    await screen.findByText("hello-from-checks", {}, { timeout: 20_000 });
  }, 40_000);

  it("filters check logs by workflow, defaulting to all checks", async () => {
    const repoPath = await openWorkspace("feat/checks-filter", true);
    await trustAndRunAll(repoPath);

    await user.click(await screen.findByRole("tab", { name: /^Logs/ }));
    const filter = await screen.findByRole("button", {
      name: "Filter by check",
    });
    expect(filter.textContent).toContain("All checks");
    expect(
      within(screen.getByTestId("logs-tab")).queryByRole("button", {
        name: /Run all/,
      }),
    ).toBeNull();

    await screen.findByText("hello-from-checks", {}, { timeout: 20_000 });
    await screen.findByText("hello-from-docs", {}, { timeout: 20_000 });

    await user.click(filter);
    await user.click(
      await screen.findByRole("menuitemcheckbox", { name: "Docs" }),
    );
    expect(filter.textContent).toContain("Docs");
    await waitFor(() =>
      expect(screen.queryByText("hello-from-checks")).toBeNull(),
    );
    expect(screen.getByText("hello-from-docs")).toBeTruthy();

    await user.click(
      await screen.findByRole("menuitemcheckbox", { name: "Logging CI" }),
    );
    expect(filter.textContent).toContain("2 checks");
    await screen.findByText("hello-from-checks", {}, { timeout: 20_000 });
    expect(screen.getByText("hello-from-docs")).toBeTruthy();

    await user.click(
      await screen.findByRole("menuitemcheckbox", { name: "Docs" }),
    );
    await user.click(
      await screen.findByRole("menuitemcheckbox", { name: "Logging CI" }),
    );
    expect(filter.textContent).toContain("All checks");
    await screen.findByText("hello-from-checks");
    await screen.findByText("hello-from-docs");
  }, 40_000);
});
