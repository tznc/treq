import { describe, expect, it } from "vitest";
import { claudeUrl, CODEX_WEB_URL, treqPrUrl, treqRepoUrl } from "./links";

describe("links", () => {
  it("prefills Claude Code on the web with the repository", () => {
    expect(claudeUrl("tznc/treq")).toBe("https://claude.ai/code?repositories=tznc%2Ftreq");
  });

  it("opens Codex on the web", () => {
    expect(CODEX_WEB_URL).toBe("https://chatgpt.com/codex");
  });

  it("deep links a repository and a pull request into Treq", () => {
    expect(treqRepoUrl("tznc/treq")).toBe("treq://repo/open?repo=tznc%2Ftreq");
    expect(treqPrUrl("tznc/treq", 42)).toBe("treq://pr/open?repo=tznc%2Ftreq&number=42");
  });
});
