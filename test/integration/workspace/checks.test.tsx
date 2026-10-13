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

    const actions = await screen.findByTestId(
      "logs-checks-actions",
      {},
      { timeout: 20_000 },
    );
    await user.click(
      await within(actions).findByRole("button", { name: "Trust Repository" }),
    );
    await waitFor(async () => expect(await isRepoTrusted(repoPath)).toBe(true));

    await user.selectOptions(
      screen.getByRole("combobox", { name: "Filter by check" }),
      "ci.yaml",
    );
    const run = await within(actions).findByRole("button", {
      name: "Run Logging CI",
    });
    await waitFor(() =>
      expect((run as HTMLButtonElement).disabled).toBe(false),
    );
    await user.click(run);

    await within(actions).findByRole(
      "button",
      { name: "Re-run Logging CI" },
      { timeout: 20_000 },
    );
    expect(
      within(screen.getByTestId("selected-check-status")).getByTestId(
        "run-status-passed",
      ),
    ).toBeTruthy();
    await screen.findByText("hello-from-checks", {}, { timeout: 20_000 });
  });

  it("filters check logs by workflow, defaulting to all checks", async () => {
    const repoPath = await openWorkspace("feat/checks-filter", true);
    await user.click(await screen.findByRole("tab", { name: /^Logs/ }));
    const actions = await screen.findByTestId(
      "logs-checks-actions",
      {},
      { timeout: 20_000 },
    );
    await user.click(
      await within(actions).findByRole("button", { name: "Trust Repository" }),
    );
    await waitFor(async () => expect(await isRepoTrusted(repoPath)).toBe(true));

    const filter = screen.getByRole("combobox", {
      name: "Filter by check",
    }) as HTMLSelectElement;
    expect(filter.value).toBe("");
    const runAll = within(actions).getByRole("button", {
      name: "Run all checks",
    });
    await waitFor(() =>
      expect((runAll as HTMLButtonElement).disabled).toBe(false),
    );
    await user.click(runAll);

    await screen.findByText("hello-from-checks", {}, { timeout: 20_000 });
    await screen.findByText("hello-from-docs", {}, { timeout: 20_000 });

    await user.selectOptions(filter, "docs.yaml");
    await waitFor(() =>
      expect(screen.queryByText("hello-from-checks")).toBeNull(),
    );
    expect(screen.getByText("hello-from-docs")).toBeTruthy();

    await user.selectOptions(filter, "");
    await screen.findByText("hello-from-checks");
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
    await screen.findByRole("tab", { name: /^Logs/, selected: true });
    expect(
      (
        (await screen.findByRole("combobox", {
          name: "Filter by check",
        })) as HTMLSelectElement
      ).value,
    ).toBe("");
    const actions = await screen.findByTestId("logs-checks-actions");
    await user.click(
      await within(actions).findByRole("button", { name: "Trust Repository" }),
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
