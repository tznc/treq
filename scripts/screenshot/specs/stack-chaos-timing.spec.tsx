import * as React from "react";
import { expect, it } from "vitest";
import userEvent from "@testing-library/user-event";
import {
  createTestRepo,
  openRepo,
} from "../../../test/utils";
import { render, screen, waitFor, within } from "../../../test/test-utils";
import { Dashboard } from "../../../src/components/Dashboard";
import { getWorkspaces } from "../../../src/lib/api";
import { captureDocument } from "../capture";

import { appendFileSync } from "node:fs";
import { clickGpuiHome, clickGpuiRow, pickGpuiRowMenuItem, stackGpui, waitForNoGpuiRow } from "../../../test/gpui-sidebar";
const chaosLog = (...a: unknown[]) =>
  appendFileSync("/tmp/claude-0/chaos.log", `${a.join(" ")}\n`);

type User = ReturnType<typeof userEvent.setup>;

async function stackFromHome(user: User, branchName: string) {
  await clickGpuiHome();
  await screen.findByTestId("show-workspace-header");
  await user.click(await screen.findByRole("button", { name: "Stack" }));
  const dialog = await screen.findByTestId("modal");
  await user.type(within(dialog).getByLabelText("Branch Name"), branchName);
  await user.click(
    within(dialog).getByRole("button", { name: "Create Workspace" }),
  );
  await waitFor(() =>
    expect(screen.queryByTestId("modal")).not.toBeInTheDocument(),
  );
}

async function stackOn(
  user: User,
  parentBranch: string,
  branchName: string,
  position: "after" | "before" = "after",
) {
  await stackGpui(parentBranch);
  const dialog = await screen.findByTestId("modal");
  if (position === "before") {
    await user.click(within(dialog).getByRole("button", { name: /^before$/i }));
  }
  await user.type(within(dialog).getByLabelText("Branch Name"), branchName);
  await user.click(
    within(dialog).getByRole("button", { name: "Create Workspace" }),
  );
  await waitFor(() =>
    expect(screen.queryByTestId("modal")).not.toBeInTheDocument(),
  );
}

async function targets(repoPath: string) {
  return Object.fromEntries(
    (await getWorkspaces(repoPath)).map((w) => [w.branch_name, w.target_branch]),
  );
}

const settle = (ms = 700) => new Promise((r) => setTimeout(r, ms));

// Chaos: double-click Create, and press Escape while creation is in flight.
it("stack chaos: double-click and escape mid-create", async () => {
  const { repoPath } = createTestRepo(false);
  openRepo(repoPath);
  const user = userEvent.setup();
  render(<Dashboard />);
  await stackFromHome(user, "tm/base");

  // Double-click the submit button.
  await stackGpui("tm/base");
  let dialog = await screen.findByTestId("modal");
  await user.type(within(dialog).getByLabelText("Branch Name"), "tm/dbl");
  await user.dblClick(
    within(dialog).getByRole("button", { name: "Create Workspace" }),
  );
  await waitFor(() =>
    expect(screen.queryByTestId("modal")).not.toBeInTheDocument(),
  );
  await settle(1500);
  chaosLog(
    "[chaos] workspaces after dblclick",
    JSON.stringify((await getWorkspaces(repoPath)).map((w) => w.branch_name)),
  );
  await captureDocument(document, {
    name: "stack-chaos-timing-01-double-click",
    expectations: [
      "Exactly one tm/dbl workspace exists and there is no 'already registered' error toast.",
    ],
  });

  // Escape while "Creating..." is showing.
  await stackGpui("tm/base");
  dialog = await screen.findByTestId("modal");
  await user.type(within(dialog).getByLabelText("Branch Name"), "tm/esc");
  await user.click(
    within(dialog).getByRole("button", { name: "Create Workspace" }),
  );
  const inFlight =
    screen.queryByRole("button", { name: /Creating/ }) !== null;
  await user.keyboard("{Escape}");
  chaosLog("[chaos] escape while creating", JSON.stringify({ inFlight }));
  // Escape is ignored while the create is in flight.
  if (inFlight) expect(screen.getByTestId("modal")).toBeInTheDocument();
  await waitFor(() =>
    expect(screen.queryByTestId("modal")).not.toBeInTheDocument(),
  );
  const header = await screen.findByTestId("show-workspace-header");
  await within(header).findByText("tm/esc");
  await settle();
  await captureDocument(document, {
    name: "stack-chaos-timing-02-escape-mid-create",
    expectations: [
      "The app shows tm/esc (created as submitted) and the sidebar nests it under tm/base.",
    ],
  });
}, 180000);

// Chaos: archive a workspace, then stack a new one reusing its name.
it("stack chaos: reuse an archived name in a stack", async () => {
  const { repoPath } = createTestRepo(false);
  openRepo(repoPath);
  const user = userEvent.setup();
  render(<Dashboard />);
  await stackFromHome(user, "re/base");
  await stackOn(user, "re/base", "re/child");

  await pickGpuiRowMenuItem("re/child", "Archive Workspace");
  await waitForNoGpuiRow("re/child");
  // Archiving the workspace on screen lands on its stack parent.
  await within(await screen.findByTestId("show-workspace-header")).findByText(
    "re/base",
  );

  await stackGpui("re/base");
  const dialog = await screen.findByTestId("modal");
  await user.type(within(dialog).getByLabelText("Branch Name"), "re/child");
  await settle(800);
  await captureDocument(document, {
    name: "stack-chaos-timing-03-archived-name-typed",
    expectations: [
      "The main view behind the dialog is re/base, not the home repo.",
      "The dialog shows the reused name exists locally without blocking submit.",
    ],
  });
  await user.click(
    within(dialog).getByRole("button", { name: "Create Workspace" }),
  );
  await settle(2500);
  chaosLog(
    "[chaos] reuse archived name",
    JSON.stringify({
      modalOpen: screen.queryByTestId("modal") !== null,
      targets: await targets(repoPath),
    }),
  );
  await captureDocument(document, {
    name: "stack-chaos-timing-04-archived-name-created",
    expectations: [
      "re/child is created again and nested under re/base in the sidebar.",
    ],
  });
}, 180000);

// Chaos: a 7-deep stack with long branch names.
it("stack chaos: deep stack with long names", async () => {
  const { repoPath } = createTestRepo(false);
  openRepo(repoPath);
  const user = userEvent.setup();
  render(<Dashboard />);
  const long = "feature/a-really-long-branch-name-that-keeps-going-and-going";
  await stackFromHome(user, `${long}-1`);
  for (let i = 2; i <= 7; i++) {
    await stackOn(user, `${long}-${i - 1}`, `${long}-${i}`);
  }
  chaosLog(
    "[chaos] deep stack descriptions",
    JSON.stringify(
      (await getWorkspaces(repoPath)).map((w) => [w.branch_name.slice(-2), (w as unknown as Record<string, unknown>).description]),
    ),
  );
  await clickGpuiRow(`${long}-4`);
  await screen.findByTestId("workspace-stack-panel");
  await settle();
  await captureDocument(document, {
    name: "stack-chaos-timing-05-deep-stack",
    expectations: [
      "Sidebar shows 7 nested rows truncated from the start, so each row's -1..-7 suffix is visible.",
      "Stack panel lists 7 entries with -4 highlighted, header reads '4 of 7'.",
    ],
  });

  await stackGpui(`${long}-7`);
  await screen.findByTestId("new-workspace-stack-card");
  await settle();
  await captureDocument(document, {
    name: "stack-chaos-timing-06-deep-stack-dialog",
    expectations: [
      "The dialog's stack card lists 7 ancestors, each truncated from the start with its -7..-1 suffix visible.",
    ],
  });
}, 300000);
