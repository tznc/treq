import { createCommit } from "../src/lib/api";
import { dispatchRefreshWorkspaceChanges } from "../src/lib/change-file-drag";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { randomUUID } from "node:crypto";
import { afterAll, expect } from "vitest";
import { act, waitFor, within } from "./test-utils";
import { waitForPendingInvokes } from "./setup.integration";

export function openRepo(repoPath: string) {
  // Point the app at a repo via the URL search param it reads
  window.history.pushState({}, "", `?repo=${encodeURIComponent(repoPath)}`);
}

type NapiTestBindings = {
  createTestRepo: (withRemote: boolean) => {
    repoPath: string;
    tempDirPath: string;
    defaultBranch: string;
  };
  cleanupTestRepo: (tempDirPath: string) => void;
  gitCommitAll: (repoPath: string, message: string) => void;
  writeWorkspaceFile: (
    workspacePath: string,
    relativePath: string,
    content: string,
    append?: boolean,
  ) => string;
  writeRepoFile: (
    repoPath: string,
    relativePath: string,
    content: string,
    append?: boolean,
  ) => string;
  resolveCommitId: (workspacePath: string, revision: string) => string;
  resolveChangeId: (workspacePath: string, revision: string) => string;
  resolveRevsetCommitIds: (workspacePath: string, revset: string) => string[];
  newCommitWithParents: (
    workspacePath: string,
    parentRevisions: string[],
  ) => string;
};

function getNapiBindings(): NapiTestBindings {
  // eslint-disable-next-line @typescript-eslint/no-require-imports
  return require("../src-tauri/target") as NapiTestBindings;
}

// Repos created by Rust's createTestRepo (the `withRemote: true` case) and
// the per-test copies made from the golden fixture below. Both are removed
// in afterAll (once per file), not afterEach: a test's async chain (SWR
// revalidation, effect cleanup, a background working-copy snapshot) can
// still be running against the repo after its own assertions finish.
// Deleting the directory under it produced an unhandled "unable to open
// database file" rejection in a *later* test. It can also trip jj-lib's
// debug-only snapshot assertion (local_working_copy.rs): a snapshot of an
// unchanged working copy first reads its tree in that assertion, and if
// the git objects are already gone the tree reads as empty.
//
// Rust-created repos go through cleanupTestRepo so the TEST_REPOS registry
// drops them; copies are removed directly since that registry only knows
// about repos it created.
const testRepoPaths = new Set<string>();
const copiedRepoDirs = new Set<string>();

afterAll(async () => {
  // Let any straggling invoke() calls against these repos finish before
  // removing the directories out from under them (see
  // waitForPendingInvokes in test/setup.integration.ts).
  await waitForPendingInvokes();
  const napi = getNapiBindings();
  for (const tempDirPath of testRepoPaths) {
    napi.cleanupTestRepo(tempDirPath);
  }
  testRepoPaths.clear();
  for (const dir of copiedRepoDirs) {
    fs.rmSync(dir, { recursive: true, force: true });
  }
  copiedRepoDirs.clear();
});

// Golden fixture for the common `withRemote: false` case: built once via the
// real NAPI path, then copied per test instead of re-running jj/git setup
// each time. A plain repo has no path baked into its own `.jj`/`.git` state
// (jj resolves the workspace root from cwd, and an uncolocated remote-free
// repo has no absolute paths in its git config), so copying the directory
// tree to a new location is safe as long as no secondary jj workspace has
// been created yet -- `createWorkspace()` calls in tests always happen after
// this copy, never before, so that stays true.
let goldenPlainRepo: {
  repoPath: string;
  tempDirPath: string;
  defaultBranch: string;
} | null = null;

function getGoldenPlainRepo() {
  if (!goldenPlainRepo) {
    goldenPlainRepo = getNapiBindings().createTestRepo(false);
  }
  return goldenPlainRepo;
}

export function createTestRepo(withRemote = false): {
  repoPath: string;
  tempDirPath: string;
  defaultBranch: string;
} {
  if (withRemote) {
    const repo = getNapiBindings().createTestRepo(true);
    testRepoPaths.add(repo.tempDirPath);
    return repo;
  }

  const golden = getGoldenPlainRepo();
  const tempDirPath = path.join(
    os.tmpdir(),
    `treq-fixture-copy-${process.pid}-${randomUUID()}`,
  );
  fs.cpSync(golden.tempDirPath, tempDirPath, { recursive: true });
  copiedRepoDirs.add(tempDirPath);

  const repoPath = path.join(
    tempDirPath,
    path.relative(golden.tempDirPath, golden.repoPath),
  );
  return { repoPath, tempDirPath, defaultBranch: golden.defaultBranch };
}

export function writeWorkspaceFile(
  workspacePath: string,
  relativePath: string,
  content: string,
  append = false,
): string {
  return getNapiBindings().writeWorkspaceFile(
    workspacePath,
    relativePath,
    content,
    append,
  );
}

export function resolveWorkspacePath(
  repoPath: string,
  workspacePath: string,
): string {
  if (path.isAbsolute(workspacePath)) {
    return workspacePath;
  }
  return path.join(repoPath, ".treq", "workspaces", workspacePath);
}

/**
 * Write `target_branch` directly in the local DB, bypassing retarget validation.
 * Use only to seed malformed graphs (e.g. self-target) for rendering tests.
 */
export function setWorkspaceTargetBranchRaw(
  repoPath: string,
  workspaceId: number,
  targetBranch: string,
): void {
  const dbPath = path.join(repoPath, ".treq", "local.db");
  const escaped = targetBranch.replace(/'/g, "''");
  execFileSync(
    "sqlite3",
    [
      dbPath,
      `UPDATE workspaces SET target_branch = '${escaped}' WHERE id = ${workspaceId};`,
    ],
    { stdio: "pipe" },
  );
}

export function seedAgentReviewComment(
  repoPath: string,
  opts: {
    /** Workspace whose diff is reviewed; ignored when `targetId` is set. */
    workspaceId?: number;
    /** Defaults to `workspace_diff`. */
    targetType?: string;
    targetId?: string;
    filePath: string;
    startLine: number;
    endLine?: number;
    side?: "old" | "new";
    commentText: string;
    suggestedReplacement?: string;
    /** Exact reviewed text, as `treq agent-review add` stores for Linear. */
    quotedText?: string;
  },
): void {
  const targetType = opts.targetType ?? "workspace_diff";
  const targetId = opts.targetId ?? String(opts.workspaceId);
  const nullable = (value: string | undefined) =>
    value === undefined ? "NULL" : `'${value.replace(/'/g, "''")}'`;
  const dbPath = path.join(repoPath, ".treq", "local.db");
  const id = randomUUID();
  const esc = (s: string) => s.replace(/'/g, "''");
  execFileSync(
    "sqlite3",
    [
      dbPath,
      `INSERT INTO agent_review_comments
        (id, repo_path, target_type, target_id, file_path, hunk_id, start_line, end_line, side, comment_text, suggested_replacement, status, source, created_at, resolved_at, quoted_text)
        VALUES ('${id}', '${esc(repoPath)}', '${esc(targetType)}', '${esc(targetId)}', '${esc(opts.filePath)}', NULL,
        ${opts.startLine}, ${opts.endLine ?? opts.startLine}, ${opts.side ? `'${opts.side}'` : "NULL"},
        '${esc(opts.commentText)}', ${opts.suggestedReplacement ? `'${esc(opts.suggestedReplacement)}'` : "NULL"},
        'open', 'local-agent', '${new Date().toISOString()}', NULL, ${nullable(opts.quotedText)});`,
    ],
    { stdio: "pipe" },
  );
}

/**
 * Resolve a revision to its short commit id via jj-lib (no `jj` CLI on PATH required).
 */
export function resolveCommitId(
  workspacePath: string,
  revision: string,
): string {
  return getNapiBindings().resolveCommitId(workspacePath, revision);
}

/**
 * Resolve a revision to its short change id. Change ids survive rewrites (rebase,
 * amend), so use this when a revision is captured before later history edits.
 */
export function resolveChangeId(
  workspacePath: string,
  revision: string,
): string {
  return getNapiBindings().resolveChangeId(workspacePath, revision);
}

/**
 * Resolve a revset to the short commit ids it matches; empty when it matches nothing.
 */
export function resolveRevsetCommitIds(
  workspacePath: string,
  revset: string,
): string[] {
  return getNapiBindings().resolveRevsetCommitIds(workspacePath, revset);
}

/**
 * Create a new working-copy commit on the given parents (equivalent to `jj new <rev>...`).
 */
export function newCommitWithParents(
  workspacePath: string,
  parentRevisions: string[],
): string {
  return getNapiBindings().newCommitWithParents(workspacePath, parentRevisions);
}

export async function writeRepoFile(
  repoPath: string,
  relativePath: string,
  content: string,
  append = false,
): Promise<string> {
  return getNapiBindings().writeRepoFile(
    repoPath,
    relativePath,
    content,
    append,
  );
}

export async function commitRepoFile(
  repoPath: string,
  relativePath: string,
  content: string,
  message: string,
): Promise<void> {
  await writeRepoFile(repoPath, relativePath, `${content}\n`, true);
  await createCommit(repoPath, null, message);
}

// Write a file and commit it with git directly, advancing the current branch
// ref (unlike commitRepoFile, which commits through jj on the home repo).
export async function gitCommitRepoFile(
  repoPath: string,
  relativePath: string,
  content: string,
  message: string,
): Promise<void> {
  await writeRepoFile(repoPath, relativePath, `${content}\n`, true);
  getNapiBindings().gitCommitAll(repoPath, message);
}

export async function commitWorkspaceFile(
  repoPath: string,
  workspace: { id: number; path: string },
  relativePath: string,
  content: string,
  message: string,
): Promise<void> {
  const workspacePath = resolveWorkspacePath(repoPath, workspace.path);
  writeWorkspaceFile(workspacePath, relativePath, `${content}\n`, true);
  await createCommit(repoPath, workspace.id, message);
}

// Helper: wait for sidebar to show branch name, then return the first matching element
export async function findSidebarBranchElement(
  branchName: string,
): Promise<HTMLElement> {
  const sidebarRoot = await waitFor(() => {
    const el = document.querySelector(
      "[data-testid='workspace-sidebar']",
    ) as HTMLElement | null;
    if (!el) {
      throw new Error("workspace sidebar not mounted");
    }
    return el;
  });
  await waitFor(() => {
    expect(within(sidebarRoot).getAllByText(branchName).length).toBeGreaterThan(
      0,
    );
  });
  return within(sidebarRoot).getAllByText(branchName)[0];
}

/**
 * A workspace's sidebar row. Its hover actions include "Start agent" and
 * "Open shell", which open terminals in that workspace.
 */
export async function findSidebarWorkspaceRow(
  branchName: string,
): Promise<HTMLElement> {
  const label = await findSidebarBranchElement(branchName);
  return label.closest("div") as HTMLElement;
}

/**
 * Stands in for the file watcher, which tests don't run: after a test writes
 * to a workspace outside the UI (files, commits), this refreshes the visible
 * tab the way the watcher's `workspace-files-changed` event does in the app.
 */
export function notifyWorkspaceChanged(workspaceId?: number) {
  act(() => {
    dispatchRefreshWorkspaceChanges(
      workspaceId === undefined ? undefined : { workspaceId },
    );
  });
}

/** The URL of the repo's `origin` remote (a path for local test remotes). */
export function getOriginUrl(repoPath: string): string {
  const config = fs.readFileSync(
    path.join(repoPath, ".git", "config"),
    "utf-8",
  );
  const match = /\[remote "origin"\][^[]*?url\s*=\s*(.+)/.exec(config);
  if (!match) throw new Error(`no origin remote in ${repoPath}`);
  return match[1].trim();
}

/** Point the repo's `origin` remote at `remoteUrl`, adding the remote if absent. */
export function setOriginUrl(repoPath: string, remoteUrl: string) {
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
