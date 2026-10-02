// @include-parallel
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { randomUUID } from "node:crypto";
import { afterEach, describe, expect, it } from "vitest";
import {
  loadFileBrowserReview,
  loadPendingReview,
  markFileViewed,
  saveFileBrowserReview,
  savePendingReview,
} from "../../src/lib/api";
import {
  activeRepositoryFromRemote,
  setActiveRepositorySingleton,
} from "../../src/lib/active-repository";
import type { SshEndpoint } from "../../src/lib/api-types-remote";

function openRemoteOnlyPath(remotePath: string) {
  const endpoint = { id: "ep-review-state" } as unknown as SshEndpoint;
  setActiveRepositorySingleton(
    activeRepositoryFromRemote(
      {
        host: "remote.invalid",
        path: remotePath,
        display_name: "remote",
        repo_uri: `ssh://remote.invalid${remotePath}`,
        endpoint_id: endpoint.id,
        endpoint_generation: 0,
      },
      endpoint,
    ),
  );
}

describe("remote review state", () => {
  afterEach(() => setActiveRepositorySingleton(null));

  it("persists review drafts without creating the remote path locally", async () => {
    const remotePath = path.join(
      os.tmpdir(),
      `treq-remote-only-${randomUUID()}`,
    );
    openRemoteOnlyPath(remotePath);

    await savePendingReview(remotePath, 4, [], [], "pending draft");
    await saveFileBrowserReview(remotePath, 4, [], "browser draft");
    await markFileViewed(`${remotePath}/.treq/workspaces/ws`, "a.rs", "h1");

    expect((await loadPendingReview(remotePath, 4))?.summary_text).toBe(
      "pending draft",
    );
    expect((await loadFileBrowserReview(remotePath, 4))?.summary_text).toBe(
      "browser draft",
    );
    expect(fs.existsSync(remotePath)).toBe(false);
  });
});
