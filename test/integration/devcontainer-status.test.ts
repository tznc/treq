// @include-parallel
import { mkdirSync, writeFileSync } from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";
import { devcontainerStatus } from "../../src/lib/api-devcontainer";
import { createTestRepo } from "../utils";

describe("devcontainer_status", () => {
  it("reports a repository without a devcontainer config", async () => {
    const { repoPath } = createTestRepo(false);
    const status = await devcontainerStatus(repoPath);
    expect(status.configured).toBe(false);
    expect(status.configPath).toBeNull();
    expect(status.running).toBe(false);
  });

  it("finds the config and reports the container stopped", async () => {
    const { repoPath } = createTestRepo(false);
    mkdirSync(path.join(repoPath, ".devcontainer"));
    writeFileSync(
      path.join(repoPath, ".devcontainer/devcontainer.json"),
      '{"image":"alpine:3.20"}',
    );
    const status = await devcontainerStatus(repoPath);
    expect(status.configured).toBe(true);
    expect(status.configPath).toBe(".devcontainer/devcontainer.json");
    expect(status.running).toBe(false);
  });
});
