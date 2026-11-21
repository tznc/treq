import { describe, expect, it } from "vitest";
import { claudeUrl, CODEX_WEB_URL, codexAppUrl, linePrompt, treqKickoffUrl, treqPrUrl, treqRepoUrl } from "./links";

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

describe("kickoff links", () => {
  it("prefills a prompt in Claude Code on the web and the Codex app", () => {
    expect(claudeUrl("tznc/treq", "fix a b")).toBe("https://claude.ai/code?repositories=tznc%2Ftreq&prompt=fix%20a%20b");
    expect(codexAppUrl("fix a b")).toBe("codex://new?prompt=fix%20a%20b");
  });

  it("starts an agent in Treq on a branch of a GitHub repository", () => {
    expect(treqKickoffUrl("tznc/treq", "claude/x", "fix")).toBe(
      "treq://agent/kickoff?repo=tznc%2Ftreq&branch=claude%2Fx&prompt=fix",
    );
  });

  it("puts the line first in the prompt", () => {
    expect(linePrompt({ repo: "tznc/treq", ref: "main", path: "src/a.rs", line: 4 }, "why?")).toBe(
      "src/a.rs:4 (tznc/treq@main)\n\nwhy?",
    );
  });
});
