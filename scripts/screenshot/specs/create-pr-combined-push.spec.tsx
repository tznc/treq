import fs from "node:fs";
import path from "node:path";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { expect, it, vi } from "vitest";
import { Dashboard } from "../../../src/components/Dashboard";
import { getWorkspaces } from "../../../src/lib/api";
import { render, screen, waitFor, within } from "../../../test/test-utils";
import {
  createTestRepo,
  openRepo,
  resolveWorkspacePath,
  writeWorkspaceFile,
} from "../../../test/utils";
import { captureDocument } from "../capture";

const BRANCH_NAME = "feat/create-pr-combined-demo";

// Deferred resolvers so the spec can hold the button in its loading state
// long enough to capture it, then finish the push and the PR creation in
// order, exactly like the real handler does.
const deferred = vi.hoisted(() => ({
  pushResolve: null as null | (() => void),
  createResolve: null as null | ((prNumber: number) => void),
}));

vi.mock("../../../src/lib/api", async () => {
  const actual = await vi.importActual<typeof import("../../../src/lib/api")>(
    "../../../src/lib/api",
  );
  return {
    ...actual,
    getCachedPrInfo: vi.fn().mockResolvedValue(null),
    startPrStatusPolling: vi.fn(async () => undefined),
    refreshPrBranchStatus: vi.fn(async () => undefined),
    stopPrStatusPolling: vi.fn(async () => undefined),
    refreshPrStatuses: vi.fn(async () => undefined),
    pushWorkspaceToRemote: vi.fn(
      () =>
        new Promise<string>((resolve) => {
          deferred.pushResolve = () => resolve("pushed");
        }),
    ),
    ghCreatePr: vi.fn(
      () =>
        new Promise<number>((resolve) => {
          deferred.createResolve = (prNumber) => resolve(prNumber);
        }),
    ),
  };
});

function setOriginUrl(repoPath: string, remoteUrl: string) {
  const configPath = path.join(repoPath, ".git", "config");
  let config = fs.readFileSync(configPath, "utf-8");
  if (/\[remote "origin"\][\s\S]*?url\s*=/.test(config)) {
    config = config.replace(
      /(\[remote "origin"\][\s\S]*?url\s*=\s*).*/m,
      `$1${remoteUrl}`,
    );
  } else {
    config += `\n[remote "origin"]\n\turl = ${remoteUrl}\n`;
  }
  fs.writeFileSync(configPath, config);
}

// Scenario: a workspace branch that has never been pushed, in a repo with a
// GitHub remote. Previously this showed "Push" and only offered
// "Create PR" as a second step after that push landed. Now "Create PR" shows
// immediately and pushes the branch itself before creating the PR, so this
// captures that combined flow: at-rest, mid-flight, and settled.
it("captures Create PR pushing an unpushed branch before creating the PR", async () => {
  const { repoPath } = createTestRepo(true);
  openRepo(repoPath);
  setOriginUrl(repoPath, "https://github.com/acme/treq.git");

  const user = userEvent.setup();
  render(<Dashboard />);

  await screen.findByTestId("show-workspace-header");
  await user.click(await screen.findByRole("button", { name: "Stack" }));
  const dialog = await screen.findByTestId("modal");
  await user.type(within(dialog).getByLabelText("Branch Name"), BRANCH_NAME);
  await user.click(
    within(dialog).getByRole("button", { name: "Create Workspace" }),
  );
  await waitFor(() => {
    expect(screen.queryByTestId("modal")).not.toBeInTheDocument();
  });

  const header = await screen.findByTestId("show-workspace-header");
  await within(header).findByText(BRANCH_NAME);

  // Create PR is disabled until the workspace has a real commit (not just
  // working-copy changes), so drive an actual commit through the Changes tab
  // -- writing the file alone wouldn't do it, and committing via the NAPI
  // helper directly would bypass the query invalidation a real commit
  // triggers, leaving the button stuck showing stale (disabled) state.
  const workspace = (await getWorkspaces(repoPath)).find(
    (w) => w.branch_name === BRANCH_NAME,
  )!;
  writeWorkspaceFile(
    resolveWorkspacePath(repoPath, workspace.workspace_path),
    "feature.txt",
    "feature content\n",
  );
  await user.click(await screen.findByRole("tab", { name: /^Changes/ }));
  await screen.findByRole("tab", { name: /^Changes/, selected: true });
  await waitFor(() =>
    expect(screen.getAllByText("feature.txt").length).toBeGreaterThan(0),
  );
  await user.type(await screen.findByPlaceholderText("Message"), "Add feature");
  await user.click(await screen.findByRole("button", { name: /^commit\b/i }));

  await user.click(await screen.findByRole("tab", { name: /^Code/ }));

  const createPrButton = await within(header).findByRole("button", {
    name: /^create pr$/i,
  });
  await waitFor(() => expect(createPrButton).toBeEnabled());
  expect(
    within(header).queryByRole("button", { name: /^push$/i }),
  ).not.toBeInTheDocument();
  await captureDocument(document, {
    name: "create-pr-combined-push-01-at-rest",
    expectations: [
      "The workspace header shows a dark 'Create PR' button with a GitHub icon -- there is no separate 'Push' button, even though this branch has never been pushed.",
    ],
  });

  await user.click(createPrButton);
  await screen.findByText("Pushing & creating…");
  await captureDocument(document, {
    name: "create-pr-combined-push-02-loading",
    expectations: [
      "The button that used to read 'Create PR' now shows a spinning loading icon and the text 'Pushing & creating…' in its place.",
      "The button and its chevron sibling both look disabled (dimmed/inactive).",
    ],
  });

  deferred.pushResolve?.();
  await waitFor(() => expect(deferred.createResolve).not.toBeNull());
  deferred.createResolve?.(77);

  await screen.findByText("Pull request created");
  await captureDocument(document, {
    name: "create-pr-combined-push-03-created",
    expectations: [
      "A green success toast reads 'Pull request created' with '#77' beneath it.",
      "The 'Create PR' button is back to its normal at-rest label (no spinner).",
    ],
  });
}, 60000);
