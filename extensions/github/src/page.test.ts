import { describe, expect, it } from "vitest";
import { currentPage, parsePage } from "./page";

describe("parsePage", () => {
  it("reads a repository home", () => {
    expect(parsePage("/tznc/treq")).toEqual({ kind: "repo", repo: "tznc/treq" });
    expect(parsePage("/tznc/treq/")).toEqual({ kind: "repo", repo: "tznc/treq" });
  });

  it("reads a pull request and its tabs", () => {
    expect(parsePage("/tznc/treq/pull/42")).toEqual({ kind: "pr", repo: "tznc/treq", number: 42 });
    expect(parsePage("/tznc/treq/pull/42/files")).toEqual({ kind: "pr", repo: "tznc/treq", number: 42 });
  });

  it("names other pages of a repository", () => {
    expect(parsePage("/tznc/treq/issues")).toEqual({ kind: "other", repo: "tznc/treq" });
  });

  it("rejects paths that are not a repository", () => {
    for (const path of ["/", "/tznc", "/tznc/..", "/a b/treq", "/tznc/treq/pull/0", "/tznc/treq/pull/x"]) {
      expect(parsePage(path), path).toBeNull();
    }
  });
});

describe("currentPage", () => {
  const withRepoMeta = (nwo: string | null) => {
    document.head.innerHTML = nwo ? `<meta name="octolytics-dimension-repository_nwo" content="${nwo}">` : "";
  };

  it("trusts the path only when GitHub marks the page as that repository", () => {
    withRepoMeta("tznc/treq");
    expect(currentPage(document, "/tznc/treq/pull/7")).toEqual({ kind: "pr", repo: "tznc/treq", number: 7 });
    expect(currentPage(document, "/TZNC/Treq")).toEqual({ kind: "repo", repo: "TZNC/Treq" });
  });

  it("ignores account pages shaped like a repository", () => {
    withRepoMeta(null);
    expect(currentPage(document, "/settings/profile")).toBeNull();
    withRepoMeta("tznc/treq");
    expect(currentPage(document, "/orgs/tznc")).toBeNull();
  });
});
