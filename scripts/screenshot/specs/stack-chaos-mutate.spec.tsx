import * as React from "react";
import { expect, it } from "vitest";
import userEvent from "@testing-library/user-event";
import {
  commitWorkspaceFile,
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

async function commitOn(repoPath: string, branch: string, message: string) {
  const ws = (await getWorkspaces(repoPath)).find(
    (w) => w.branch_name === branch,
  );
  if (!ws) throw new Error(`missing ${branch}`);
  await commitWorkspaceFile(
    repoPath,
    { id: ws.id, path: ws.workspace_path },
    `${branch.replace(/\//g, "-")}.txt`,
    message,
    message,
  );
}

async function targets(repoPath: string) {
  return Object.fromEntries(
    (await getWorkspaces(repoPath)).map((w) => [w.branch_name, w.target_branch]),
  );
}

const settle = () => new Promise((r) => setTimeout(r, 700));

// Chaos: archive the middle of A -> B -> C. What does C end up stacked on?
it("stack chaos: archiving the middle of a stack", async () => {
  const { repoPath } = createTestRepo(false);
  openRepo(repoPath);
  const user = userEvent.setup();
  render(<Dashboard />);

  await stackFromHome(user, "mid/a");
  await commitOn(repoPath, "mid/a", "A commit");
  await stackOn(user, "mid/a", "mid/b");
  await commitOn(repoPath, "mid/b", "B commit");
  await stackOn(user, "mid/b", "mid/c");
  await commitOn(repoPath, "mid/c", "C commit");

  await pickGpuiRowMenuItem("mid/b", "Archive Workspace");
  await waitForNoGpuiRow("mid/b");
  const t = await targets(repoPath);
  chaosLog("[chaos] targets after archiving middle", JSON.stringify(t));
  // mid/c is spliced onto mid/b's parent, not dropped onto the default branch.
  expect(t["mid/c"]).toBe("mid/a");

  await clickGpuiRow("mid/c");
  await user.click(await screen.findByRole("tab", { name: /^Commits/ }));
  await screen.findByText("C commit");
  await settle();
  await captureDocument(document, {
    name: "stack-chaos-mutate-01-archive-middle",
    expectations: [
      "mid/c stays stacked under mid/a in the sidebar after mid/b is archived.",
      "The header target for mid/c names its real parent, not the default branch.",
      "Commits tab for mid/c lists C commit and B commit as its own; A commit is not in its list.",
    ],
  });
}, 180000);

// Chaos: "Before" on the stack root, then reorder the root onto its grandchild.
it("stack chaos: before on the root and a grandchild reorder", async () => {
  const { repoPath, defaultBranch } = createTestRepo(false);
  openRepo(repoPath);
  const user = userEvent.setup();
  render(<Dashboard />);

  await stackFromHome(user, "ro/a");
  await stackOn(user, "ro/a", "ro/b");
  await stackOn(user, "ro/b", "ro/c");
  // "Before" on the stack root inserts a new base under the whole stack.
  await stackOn(user, "ro/a", "ro/root", "before");

  let t = await targets(repoPath);
  chaosLog("[chaos] targets after before-on-root", JSON.stringify(t));
  expect(t["ro/root"]).toBe(defaultBranch);
  expect(t["ro/a"]).toBe("ro/root");
  expect(t["ro/b"]).toBe("ro/a");

  await clickGpuiRow("ro/c");
  await screen.findByTestId("workspace-stack-panel");
  await settle();
  await captureDocument(document, {
    name: "stack-chaos-mutate-02-before-root",
    expectations: [
      "Sidebar nests ro/root > ro/a > ro/b > ro/c.",
      "Stack panel shows 4 workspaces above the default branch with ro/c highlighted.",
    ],
  });

  // Reorder: move ro/a onto its grandchild ro/c.
  await clickGpuiRow("ro/a");
  const targetBtn = await screen.findByRole("button", {
    name: "Workspace target",
  });
  await waitFor(() => expect(targetBtn).not.toBeDisabled());
  await user.click(targetBtn);
  await user.click(
    await screen.findByText("ro/c", { selector: ".branch-list-item *" }),
  );
  await waitFor(async () => {
    expect((await targets(repoPath))["ro/a"]).toBe("ro/c");
  });
  t = await targets(repoPath);
  chaosLog("[chaos] targets after grandchild reorder", JSON.stringify(t));
  await settle();
  await captureDocument(document, {
    name: "stack-chaos-mutate-03-grandchild-reorder",
    expectations: [
      "Every workspace is still visible in the sidebar (none vanished).",
      "The tree is acyclic: ro/a now sits below ro/c.",
    ],
  });
}, 180000);
