// @include-parallel
import { describe, expect, it } from "vitest";
import { dispatchLocal } from "../../src/lib/remote-dispatch";
import { createTestRepo, setOriginUrl } from "../utils";

describe("remote git remote info command", () => {
  it("reads the GitHub owner and name from the origin remote", async () => {
    const { repoPath } = createTestRepo(true);
    setOriginUrl(repoPath, "git@github.com:acme/widgets.git");

    await expect(
      dispatchLocal({ kind: "GitRemoteInfo", repo: repoPath }),
    ).resolves.toEqual({
      owner: "acme",
      repo: "widgets",
      full_name: "acme/widgets",
    });
  });
});
