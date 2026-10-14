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
import { clickGpuiHome, clickGpuiRow, stackGpui } from "../../../test/gpui-sidebar";
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

async function openStackDialogOn(parentBranch: string) {
  await stackGpui(parentBranch);
  return screen.findByTestId("modal");
}

async function stackOn(
  user: User,
  parentBranch: string,
  branchName: string,
  position: "after" | "before" = "after",
) {
  const dialog = await openStackDialogOn(parentBranch);
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

// Chaos: build A -> B, insert X *before* B, then stack C on B, and check the
// sidebar tree and the stored targets agree with what the dialog promised.
it("stack chaos: before-insert keeps the chain linear", async () => {
  const { repoPath, defaultBranch } = createTestRepo(false);
  openRepo(repoPath);
  const user = userEvent.setup();
  render(<Dashboard />);

  await stackFromHome(user, "chaos/a");
  await stackOn(user, "chaos/a", "chaos/b");
  await stackOn(user, "chaos/b", "chaos/x", "before");
  await stackOn(user, "chaos/b", "chaos/c");

  const t = await targets(repoPath);
  chaosLog("[chaos] targets after before-insert", JSON.stringify(t));
  expect(t["chaos/a"]).toBe(defaultBranch);
  expect(t["chaos/x"]).toBe("chaos/a");
  expect(t["chaos/b"]).toBe("chaos/x");
  expect(t["chaos/c"]).toBe("chaos/b");

  // No auto-generated "Stacked from <parent>" description to go stale.
  const described = (await getWorkspaces(repoPath)).filter(
    (w) => w.description,
  );
  expect(described).toEqual([]);

  await clickGpuiRow("chaos/b");
  await screen.findByTestId("workspace-stack-panel");
  await new Promise((r) => setTimeout(r, 500));
  await captureDocument(document, {
    name: "stack-chaos-create-01-before-insert",
    expectations: [
      "Sidebar nests chaos/a > chaos/x > chaos/b > chaos/c, each one indent deeper.",
      "The header has no 'Stacked from chaos/a' line under chaos/b.",
      "The Stack panel lists chaos/c, chaos/b (highlighted), chaos/x, chaos/a above the default branch.",
    ],
  });
}, 120000);

// Chaos: hostile branch names typed into the stack dialog on a workspace.
it("stack chaos: invalid and duplicate branch names", async () => {
  const { repoPath } = createTestRepo(false);
  openRepo(repoPath);
  const user = userEvent.setup();
  render(<Dashboard />);

  await stackFromHome(user, "chaos/parent");

  // 1. Stack onto chaos/parent using chaos/parent's own name.
  const dialog = await openStackDialogOn("chaos/parent");
  const input = within(dialog).getByLabelText("Branch Name");
  const submit = within(dialog).getByRole("button", {
    name: "Create Workspace",
  });
  await user.type(input, "chaos/parent");
  expect(await within(dialog).findByTestId("branch-name-error")).toHaveTextContent(
    "A workspace for chaos/parent already exists",
  );
  expect(submit).toBeDisabled();
  await new Promise((r) => setTimeout(r, 800));
  await captureDocument(document, {
    name: "stack-chaos-create-02-self-name-typed",
    expectations: [
      "A red error under the input says a workspace for chaos/parent already exists.",
      "The Create Workspace button is disabled.",
    ],
  });

  // 2. Whitespace and illegal characters.
  await user.clear(input);
  await user.type(input, "  chaos/has space  ");
  expect(await within(dialog).findByTestId("branch-name-error")).toHaveTextContent(
    "Invalid branch name: contains ' '",
  );
  expect(submit).toBeDisabled();
  await new Promise((r) => setTimeout(r, 800));
  await captureDocument(document, {
    name: "stack-chaos-create-04-space-name",
    expectations: [
      "A red inline error rejects the space in the branch name; no green check is shown.",
      "No error toast appears, and Create Workspace is disabled.",
    ],
  });

  // 3. Unicode / emoji name.
  await user.clear(input);
  await user.type(input, "chaos/ünïcødé-🚀");
  expect(within(dialog).queryByTestId("branch-name-error")).toBeNull();
  await user.click(submit);
  await new Promise((r) => setTimeout(r, 2500));
  const names = (await getWorkspaces(repoPath)).map((w) => w.branch_name);
  chaosLog("[chaos] workspaces after hostile names", JSON.stringify(names));
  await captureDocument(document, {
    name: "stack-chaos-create-05-unicode-name",
    expectations: [
      "The unicode workspace is created, selected, and nested under chaos/parent.",
    ],
  });
}, 120000);
