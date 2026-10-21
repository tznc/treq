// @include-parallel
import * as React from "react";
import { invoke } from "@tauri-apps/api/core";
import { ask } from "@tauri-apps/plugin-dialog";
import { act } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within } from "../test-utils";
import { createTestRepo, openRepo } from "../utils";
import {
  addSupportingRepo,
  createWorkspace,
  listSupportingRepos,
} from "../../src/lib/api";
import { Dashboard } from "../../src/components/Dashboard";
import type {
  GpuiSidebarAction,
  GpuiSidebarRepo,
  GpuiSidebarSnapshot,
} from "../../src/lib/gpui-shell";
import { popupNativeMenu } from "../../src/lib/native-context-menu";
import { emitOnTestBus } from "../test-event-bus";

vi.mock("../../src/lib/native-context-menu", async (importOriginal) => {
  const actual =
    await importOriginal<typeof import("../../src/lib/native-context-menu")>();
  return { ...actual, popupNativeMenu: vi.fn() };
});

function lastSnapshot(): GpuiSidebarSnapshot | null | undefined {
  const calls = vi
    .mocked(invoke)
    .mock.calls.filter(([cmd]) => cmd === "gpui_sidebar_update");
  return (calls.at(-1)?.[1] as { snapshot: GpuiSidebarSnapshot | null })
    ?.snapshot;
}

function section(path: string): GpuiSidebarRepo | undefined {
  return lastSnapshot()?.repos.find((r) => r.path === path);
}

function send(action: GpuiSidebarAction) {
  act(() => {
    emitOnTestBus("gpui-sidebar-action", action);
  });
}

async function menu(action: GpuiSidebarAction) {
  vi.mocked(popupNativeMenu).mockClear();
  send(action);
  await waitFor(() => expect(popupNativeMenu).toHaveBeenCalledOnce());
  const [[entries]] = vi.mocked(popupNativeMenu).mock.calls;
  return entries;
}

function texts(entries: Awaited<ReturnType<typeof menu>>) {
  return entries.flatMap((e) => (e.kind === "separator" ? [] : [e.text]));
}

describe("Dashboard - GPUI sidebar with supporting repositories", () => {
  let mainPath: string;
  let supportingPath: string;

  beforeEach(async () => {
    window.__TREQ_GPUI_SHELL__ = true;
    ({ repoPath: mainPath } = createTestRepo(false));
    const { repoPath: otherPath } = createTestRepo(false);
    supportingPath = await addSupportingRepo(mainPath, otherPath);
    openRepo(mainPath);
    await createWorkspace(mainPath, "feat/main-work");
    await createWorkspace(supportingPath, "feat/supporting-work");
    vi.mocked(popupNativeMenu).mockClear();
  });

  afterEach(() => {
    delete window.__TREQ_GPUI_SHELL__;
  });

  async function renderSections() {
    render(<Dashboard />);
    await waitFor(
      () =>
        expect(section(supportingPath)?.workspaces.map((w) => w.title)).toEqual(
          ["feat/supporting-work"],
        ),
      { timeout: 10_000 },
    );
  }

  it("draws a labelled section for each repository", async () => {
    await renderSections();
    const { repos } = lastSnapshot()!;
    expect(repos.map((r) => r.path)).toEqual([mainPath, supportingPath]);
    expect(repos.map((r) => r.active)).toEqual([true, false]);
    expect(repos.every((r) => r.label)).toBe(true);
    expect(repos[0].workspaces.map((w) => w.title)).toEqual(["feat/main-work"]);
  });

  it("opens a workspace of the supporting repository", async () => {
    await renderSections();
    const [{ id }] = section(supportingPath)!.workspaces;
    send({ kind: "openRepoWorkspace", repoPath: supportingPath, id });

    const header = await screen.findByTestId("show-workspace-header");
    expect(
      await within(header).findByText("feat/supporting-work"),
    ).toBeTruthy();
    await waitFor(() => expect(section(supportingPath)?.active).toBe(true));
    expect(section(mainPath)?.active).toBe(false);
  });

  it("renames a supporting workspace from its menu, without archive", async () => {
    await renderSections();
    const [{ id }] = section(supportingPath)!.workspaces;
    const entries = await menu({
      kind: "repoWorkspaceMenu",
      repoPath: supportingPath,
      id,
    });
    expect(texts(entries)).not.toContain("Archive Workspace");
    const rename = entries.find(
      (e) => e.kind === "item" && e.text === "Rename Workspace",
    );
    if (rename?.kind !== "item") throw new Error("no rename item");
    act(() => rename.onSelect());
    expect(
      await screen.findByText("Rename the branch for this workspace"),
    ).toBeInTheDocument();
  });

  it("folds a repository and remembers it", async () => {
    await renderSections();
    send({ kind: "toggleRepo", repoPath: supportingPath });
    await waitFor(() => expect(section(supportingPath)?.collapsed).toBe(true));
    expect(section(mainPath)?.collapsed).toBe(false);
    expect(
      localStorage.getItem(`treq.sidebar.collapsedRepos:${mainPath}`),
    ).toContain(supportingPath);

    send({ kind: "toggleRepo", repoPath: supportingPath });
    await waitFor(() => expect(section(supportingPath)?.collapsed).toBe(false));
  });

  it("offers repository actions on the supporting home row", async () => {
    await renderSections();
    const supporting = texts(
      await menu({ kind: "homeMenu", repoPath: supportingPath }),
    );
    expect(supporting).toContain("Repository Settings");
    expect(supporting).toContain("Remove Repository");
    expect(supporting).not.toContain("Add Repository…");
    expect(supporting).not.toContain("Switch Branch...");

    const main = texts(await menu({ kind: "homeMenu", repoPath: mainPath }));
    expect(main).toContain("Add Repository…");
    expect(main).toContain("Switch Branch...");
    expect(main).not.toContain("Remove Repository");
  });

  it("removes a supporting repository from its home row menu", async () => {
    vi.mocked(ask).mockResolvedValueOnce(true);
    await renderSections();
    const entries = await menu({ kind: "homeMenu", repoPath: supportingPath });
    const remove = entries.find(
      (e) => e.kind === "item" && e.text === "Remove Repository",
    );
    if (remove?.kind !== "item") throw new Error("no remove item");
    act(() => remove.onSelect());

    await waitFor(async () =>
      expect(await listSupportingRepos(mainPath)).toEqual([]),
    );
    await waitFor(() =>
      expect(lastSnapshot()?.repos.map((r) => r.path)).toEqual([mainPath]),
    );
    expect(section(mainPath)?.label).toBeNull();
  });
});
