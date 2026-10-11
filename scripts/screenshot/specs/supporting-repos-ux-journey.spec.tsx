import userEvent from "@testing-library/user-event";
import { open } from "@tauri-apps/plugin-dialog";
import * as React from "react";
import { expect, it, vi } from "vitest";
import { Dashboard } from "../../../src/components/Dashboard";
import { addSupportingRepo, createWorkspace } from "../../../src/lib/api";
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

// The everyday entry points a multi-repository user reaches for, beyond the
// sidebar itself: discovering "Add Repository…" in a single-repository
// window, the Cmd+K workspace picker, the Cmd+I agent dialog, the header of
// a supporting repository, and its Repository Settings.
it("captures the multi-repository UX journey", async () => {
  const { repoPath: mainPath } = createTestRepo(false);
  const { repoPath: otherPath } = createTestRepo(false);
  openRepo(mainPath);
  await createWorkspace(mainPath, "feat/main-work");

  const user = userEvent.setup();
  const view = render(<Dashboard />);
  const sidebar = await screen.findByTestId("workspace-sidebar");
  await within(sidebar).findByText("feat/main-work");

  // Discovery: a single-repository window offers "Add Repository…" only in
  // the home row's context menu (and the File menu).
  await user.pointer({ keys: "[MouseRight]", target: await homeRow(mainPath) });
  await screen.findByText("Add Repository…");
  await captureDocument(document, {
    name: "supporting-repos-ux-01-discover-add",
    expectations: [
      "A single home row; its context menu lists Add Repository… below the copy-path items.",
    ],
  });
  vi.mocked(open).mockResolvedValueOnce(otherPath);
  await user.click(screen.getByText("Add Repository…"));
  await screen.findByText("Repository Added");
  const supportingPath = await addSupportingRepo(mainPath, otherPath).catch(
    () => null,
  );
  expect(supportingPath).toBeNull(); // already linked by the menu
  const supportingRow = await waitFor(() => {
    const rows = within(sidebar).getAllByTestId("home-repo-row");
    expect(rows).toHaveLength(2);
    return rows[1];
  });
  const svcPath = supportingRow.getAttribute("data-repo-path") ?? "";
  await createWorkspace(svcPath, "feat/svc");

  // Cmd+K > Go to workspace, with the main repository on screen.
  await user.keyboard("{Meta>}k{/Meta}");
  await user.click(await screen.findByText("Go to workspace"));
  await screen.findByPlaceholderText("Search workspaces...");
  await captureDocument(document, {
    name: "supporting-repos-ux-02-cmdk-workspaces",
    expectations: [
      "The Go to Workspace picker lists the main repository's workspaces.",
      "Whether feat/svc (supporting repository) is listed: it should be, for a user switching by keyboard.",
    ],
  });

  // Cmd+I agent dialog, main repository on screen.
  await user.keyboard("{Meta>}i{/Meta}");
  const dialog = (
    await screen.findByRole("heading", { name: "Start a new agent session" })
  ).closest('[data-testid="modal"]') as HTMLElement;
  await user.click(
    dialog.querySelector('button[role="combobox"]') as HTMLElement,
  );
  await captureDocument(document, {
    name: "supporting-repos-ux-03-cmdi-picker",
    expectations: [
      "The agent dialog's branch picker is open and lists the main repository's branches.",
      "Whether the supporting repository's feat/svc is offered, and whether options say which repository they belong to.",
    ],
  });
  // The harness mocks popovers, so Escape does not dismiss these dialogs;
  // a fresh render stands in for closing them.
  view.unmount();
  render(<Dashboard />);
  const sidebarAgain = await screen.findByTestId("workspace-sidebar");
  await within(sidebarAgain).findByText("feat/svc");
  const supportingRowAgain =
    within(sidebarAgain).getAllByTestId("home-repo-row")[1];

  // A supporting repository on screen: what tells the user which one it is?
  await user.click(supportingRowAgain);
  await within(sidebarAgain).findByText("feat/svc");
  await captureDocument(document, {
    name: "supporting-repos-ux-04-supporting-home",
    expectations: [
      "The supporting home row is highlighted.",
      "Whether the header names the repository on screen, or only its branch.",
    ],
  });

  // Repository Settings for the supporting repository.
  await user.pointer({ keys: "[MouseRight]", target: supportingRowAgain });
  await user.click(await screen.findByText("Repository Settings"));
  await screen.findByRole("heading", { name: "Settings" });
  await captureDocument(document, {
    name: "supporting-repos-ux-05-repo-settings",
    expectations: [
      "The Settings page is open.",
      "Whether the page says which repository's settings are shown.",
    ],
  });
  await user.click(screen.getByRole("button", { name: "Close" }));
}, 120000);
