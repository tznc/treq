import * as fs from "node:fs";
import userEvent from "@testing-library/user-event";
import { ask, open } from "@tauri-apps/plugin-dialog";
import * as React from "react";
import { expect, it, onTestFinished, vi } from "vitest";
import { Dashboard } from "../../../src/components/Dashboard";
import {
  createWorkspace,
  getSessions,
  getWorkspaces,
  listSupportingRepos,
} from "../../../src/lib/api";
import { installFakeAgents } from "../../../test/fake-agent";
import { render, screen, waitFor, within } from "../../../test/test-utils";
import { createTestRepo, openRepo } from "../../../test/utils";
import { captureDocument } from "../capture";

const homeRow = (repoPath: string) =>
  waitFor(() => {
    const row = document.querySelector(
      `[data-testid="home-repo-row"][data-repo-path="${CSS.escape(repoPath)}"]`,
    ) as HTMLElement | null;
    if (!row) throw new Error(`no home row for ${repoPath}`);
    return row;
  });

const rightClick = (
  user: ReturnType<typeof userEvent.setup>,
  target: HTMLElement,
) => user.pointer({ keys: "[MouseRight]", target });

// Walks the multi-repository window end to end: add a supporting repository
// from the main home row, switch to it, stack a workspace there through the
// real dialog, start an agent in it, collapse its group, and finally handle
// the folder going missing and removing it.
it("captures the supporting repositories flow", async () => {
  onTestFinished(installFakeAgents());
  const { repoPath: mainPath } = createTestRepo(false);
  const { repoPath: otherPath } = createTestRepo(false);
  openRepo(mainPath);
  await createWorkspace(mainPath, "feat/main-work");

  const user = userEvent.setup();
  const view = render(<Dashboard />);
  const sidebar = await screen.findByTestId("workspace-sidebar");
  await within(sidebar).findByText("feat/main-work");

  // Add the supporting repository from the main home row's context menu.
  vi.mocked(open).mockResolvedValueOnce(otherPath);
  await rightClick(user, await homeRow(mainPath));
  await user.click(await screen.findByText("Add Repository…"));
  await screen.findByText("Repository Added");
  const [supporting] = await listSupportingRepos(mainPath);
  const supportingPath = supporting.path;
  await homeRow(supportingPath);
  await waitFor(() =>
    expect(
      within(sidebar).getAllByTestId("repo-workspace-group-label"),
    ).toHaveLength(2),
  );
  await captureDocument(document, {
    name: "supporting-repos-01-added",
    expectations: [
      "The sidebar shows two home repository rows; only the main (on-screen) row shows the agent, shell and stack icons.",
      "A 'Workspaces' header sits above two labelled repository groups with no divider between the groups; the main group lists feat/main-work.",
      "A success toast says the repository is now a supporting repository.",
    ],
  });

  // Switch to the supporting repository and stack a workspace in it.
  await user.click(await homeRow(supportingPath));
  await screen.findByTestId("show-workspace-header");
  await user.click(await screen.findByRole("button", { name: "Stack" }));
  const dialog = await screen.findByTestId("modal");
  await user.type(within(dialog).getByLabelText("Branch Name"), "feat/svc");
  await captureDocument(document, {
    name: "supporting-repos-02-stack-dialog",
    expectations: [
      "The Stack dialog is open with feat/svc typed as the branch name.",
      "Behind the dialog, the supporting repository's home row is the highlighted row in the sidebar.",
    ],
  });
  await user.click(
    within(dialog).getByRole("button", { name: "Create Workspace" }),
  );
  await waitFor(() =>
    expect(screen.queryByTestId("modal")).not.toBeInTheDocument(),
  );
  await within(sidebar).findByText("feat/svc");
  expect(
    (await getWorkspaces(supportingPath)).map((w) => w.branch_name),
  ).toEqual(["feat/svc"]);
  expect((await getWorkspaces(mainPath)).map((w) => w.branch_name)).toEqual([
    "feat/main-work",
  ]);
  await captureDocument(document, {
    name: "supporting-repos-03-workspace-created",
    expectations: [
      "feat/svc appears under the supporting repository's group, not under the main group.",
      "The main group still lists only feat/main-work.",
    ],
  });

  // Start an agent from the supporting workspace's sidebar row.
  const svcRow = within(sidebar).getByText("feat/svc");
  await user.hover(svcRow);
  const svcItem = svcRow.closest("li") as HTMLElement;
  await user.click(within(svcItem).getByLabelText("Start agent"));
  await screen.findByText(/fake-agent:/, undefined, { timeout: 20000 });
  await waitFor(async () =>
    expect((await getSessions(supportingPath)).length).toBe(1),
  );
  expect(await getSessions(mainPath)).toEqual([]);
  await captureDocument(document, {
    name: "supporting-repos-04-agent-started",
    expectations: [
      "The workspace view for feat/svc shows a terminal running the fake agent.",
      "feat/svc is the highlighted row in the supporting repository's group.",
    ],
  });

  // Start an agent in a main workspace too. Session ids are per repository,
  // so both sessions are id 1; each workspace must keep its own terminal.
  const mainRow = within(sidebar).getByText("feat/main-work");
  await user.click(mainRow);
  await within(screen.getByTestId("show-workspace-header")).findByText(
    "feat/main-work",
  );
  await user.hover(mainRow);
  await user.click(
    within(mainRow.closest("li") as HTMLElement).getByLabelText("Start agent"),
  );
  await waitFor(async () =>
    expect((await getSessions(mainPath)).length).toBe(1),
  );
  expect((await getSessions(mainPath))[0].id).toBe(
    (await getSessions(supportingPath))[0].id,
  );
  // The pane shows both repositories' agents side by side.
  await waitFor(
    () => expect(screen.getAllByText(/fake-agent:/)).toHaveLength(2),
    { timeout: 20000 },
  );
  await captureDocument(document, {
    name: "supporting-repos-04b-main-agent",
    expectations: [
      "The terminal pane shows two agent terminals, tagged feat/svc and feat/main-work, each running the fake agent.",
      "feat/main-work is highlighted in the main group; feat/svc is not highlighted.",
    ],
  });
  await user.click(within(sidebar).getByText("feat/svc"));
  await within(screen.getByTestId("show-workspace-header")).findByText(
    "feat/svc",
  );
  // Switching repositories keeps both terminals and their output.
  expect(screen.getAllByText(/fake-agent:/)).toHaveLength(2);
  await captureDocument(document, {
    name: "supporting-repos-04c-back-to-supporting-agent",
    expectations: [
      "The header shows feat/svc, and both agent terminals are still shown with their fake-agent output.",
      "feat/svc is highlighted; feat/main-work is not.",
    ],
  });

  // Clicking the main workspace's terminal badge switches to its repository.
  const mainBadge = screen
    .getAllByText("feat/main-work")
    .find(
      (el) =>
        !sidebar.contains(el) &&
        !el.closest("[data-testid='show-workspace-header']"),
    );
  expect(mainBadge).toBeTruthy();
  await user.click(mainBadge as HTMLElement);
  await within(screen.getByTestId("show-workspace-header")).findByText(
    "feat/main-work",
  );

  // Back to the main repository, then collapse the supporting group.
  await user.click(await homeRow(mainPath));
  await waitFor(() =>
    expect(
      within(screen.getByTestId("show-workspace-header")).queryByText(
        "feat/svc",
      ),
    ).toBeNull(),
  );
  const labels = within(sidebar).getAllByTestId("repo-workspace-group-label");
  await user.click(labels[1]);
  await waitFor(() =>
    expect(within(sidebar).getByText("feat/svc")).not.toBeVisible(),
  );
  await captureDocument(document, {
    name: "supporting-repos-05-group-collapsed",
    expectations: [
      "The main home row is highlighted again.",
      "The supporting repository's group is collapsed (chevron pointing right) and feat/svc is hidden.",
    ],
  });

  // The supporting folder disappears: its row shows as missing.
  const movedPath = `${supportingPath}-moved`;
  fs.renameSync(supportingPath, movedPath);
  onTestFinished(() => {
    if (fs.existsSync(movedPath)) fs.renameSync(movedPath, supportingPath);
  });
  // The list refreshes on window focus; reopening the window stands in.
  view.unmount();
  render(<Dashboard />);
  await screen.findByText("missing", undefined, { timeout: 15000 });
  await rightClick(user, await homeRow(supportingPath));
  await screen.findByText("Locate…");
  await captureDocument(document, {
    name: "supporting-repos-06-missing",
    expectations: [
      "The supporting home row is dimmed and marked missing.",
      "Its context menu offers Locate… and Remove Repository, without the copy-path items.",
    ],
  });

  // Remove it.
  vi.mocked(ask).mockResolvedValueOnce(true);
  await user.click(await screen.findByText("Remove Repository"));
  await waitFor(async () =>
    expect(await listSupportingRepos(mainPath)).toEqual([]),
  );
  await waitFor(() =>
    expect(
      document.querySelector(
        `[data-testid="home-repo-row"][data-repo-path="${CSS.escape(supportingPath)}"]`,
      ),
    ).toBeNull(),
  );
  await captureDocument(document, {
    name: "supporting-repos-07-removed",
    expectations: [
      "Only the main home row remains, and workspaces are no longer grouped under repository labels.",
      "feat/main-work is still listed.",
    ],
  });
}, 120000);
