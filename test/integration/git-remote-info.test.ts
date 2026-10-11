// @include-parallel
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { renderHook, waitFor } from "@testing-library/react";
import React from "react";
import { SWRConfig } from "swr";
import { afterEach, describe, expect, it } from "vitest";
import { useGitRemoteInfo } from "../../src/hooks/useMergeQueueStatus";
import { SWRMutateScope, testSWRConfig } from "../../src/lib/swr-cache";

function makeWrapper() {
  return ({ children }: { children: React.ReactNode }) =>
    React.createElement(
      SWRConfig,
      { value: testSWRConfig },
      React.createElement(SWRMutateScope, null, children),
    );
}

function makeGitDir(remoteUrl?: string): string {
  const repoPath = fs.mkdtempSync(path.join(os.tmpdir(), "treq-remote-"));
  fs.mkdirSync(path.join(repoPath, ".git"));
  if (remoteUrl) {
    fs.writeFileSync(
      path.join(repoPath, ".git", "config"),
      `[remote "origin"]\n\turl = ${remoteUrl}\n`,
    );
  } else {
    fs.writeFileSync(path.join(repoPath, ".git", "config"), "");
  }
  return repoPath;
}

describe("useGitRemoteInfo", () => {
  const tempDirs: string[] = [];

  afterEach(() => {
    for (const dir of tempDirs.splice(0)) {
      fs.rmSync(dir, { recursive: true, force: true });
    }
  });

  it("returns null when repo has no .git directory", async () => {
    const repoPath = fs.mkdtempSync(path.join(os.tmpdir(), "treq-nongit-"));
    tempDirs.push(repoPath);

    const { result } = renderHook(() => useGitRemoteInfo(repoPath), {
      wrapper: makeWrapper(),
    });
    await waitFor(() => {
      expect(result.current.isLoading).toBe(false);
      expect(result.current.error).toBeUndefined();
    });
    expect(result.current.data).toBeNull();
  });

  it("returns null when origin is not a GitHub URL", async () => {
    const repoPath = makeGitDir("https://gitlab.com/owner/repo.git");
    tempDirs.push(repoPath);

    const { result } = renderHook(() => useGitRemoteInfo(repoPath), {
      wrapper: makeWrapper(),
    });
    await waitFor(() => {
      expect(result.current.isLoading).toBe(false);
      expect(result.current.error).toBeUndefined();
    });
    expect(result.current.data).toBeNull();
  });

  it("parses SSH GitHub remote from .git/config", async () => {
    const repoPath = makeGitDir("git@github.com:ziinc/treq.git");
    tempDirs.push(repoPath);

    const { result } = renderHook(() => useGitRemoteInfo(repoPath), {
      wrapper: makeWrapper(),
    });
    await waitFor(() => {
      expect(result.current.isLoading).toBe(false);
      expect(result.current.error).toBeUndefined();
    });
    expect(result.current.data).toMatchObject({
      owner: "ziinc",
      repo: "treq",
      full_name: "ziinc/treq",
    });
  });

  it("parses HTTPS GitHub remote from .git/config", async () => {
    const repoPath = makeGitDir("https://github.com/ziinc/treq.git");
    tempDirs.push(repoPath);

    const { result } = renderHook(() => useGitRemoteInfo(repoPath), {
      wrapper: makeWrapper(),
    });
    await waitFor(() => {
      expect(result.current.isLoading).toBe(false);
      expect(result.current.error).toBeUndefined();
    });
    expect(result.current.data?.full_name).toBe("ziinc/treq");
  });

  it("is disabled when repoPath is undefined", () => {
    const { result } = renderHook(() => useGitRemoteInfo(undefined), {
      wrapper: makeWrapper(),
    });
    expect(result.current.isLoading).toBe(false);
    expect(result.current.isValidating).toBe(false);
  });
});
