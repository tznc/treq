import { beforeEach, describe, expect, it } from "vitest";
import { renderOpenIn } from "./sidebar";

const links = () =>
  [...document.querySelectorAll<HTMLAnchorElement>("#treq-open-in a")].map((a) => [a.textContent, a.getAttribute("href")]);

describe("renderOpenIn", () => {
  beforeEach(() => {
    document.body.innerHTML = "";
  });

  it("adds the links to the end of a repository's sidebar", () => {
    document.body.innerHTML = `<div data-component="SplitPageLayout.Pane"><div id="grid">
      <div><h2>About</h2></div><div><h2>Languages</h2></div></div></div>`;
    renderOpenIn(document, { kind: "repo", repo: "tznc/treq" });
    expect(document.querySelector("#grid")?.lastElementChild?.id).toBe("treq-open-in");
    expect(links()).toEqual([
      ["Open in Claude", "https://claude.ai/code?repositories=tznc%2Ftreq"],
      ["Open in Codex", "https://chatgpt.com/codex"],
      ["Open in Treq", "treq://repo/open?repo=tznc%2Ftreq"],
    ]);
  });

  it("adds a sidebar item to a pull request, opening it in Treq", () => {
    document.body.innerHTML = `<div id="partial-discussion-sidebar"><div class="discussion-sidebar-item"></div></div>`;
    renderOpenIn(document, { kind: "pr", repo: "tznc/treq", number: 42 });
    const section = document.querySelector("#partial-discussion-sidebar")?.lastElementChild;
    expect(section?.id).toBe("treq-open-in");
    expect(section?.classList.contains("discussion-sidebar-item")).toBe(true);
    expect(links()[2]).toEqual(["Open in Treq", "treq://pr/open?repo=tznc%2Ftreq&number=42"]);
  });

  it("floats the links bottom right when the page has no sidebar", () => {
    renderOpenIn(document, { kind: "repo", repo: "tznc/treq" });
    const section = document.querySelector("#treq-open-in");
    expect(section?.parentElement).toBe(document.body);
    expect(section?.classList.contains("treq-open-in--floating")).toBe(true);
  });

  it("renders once per page and follows navigation", () => {
    renderOpenIn(document, { kind: "repo", repo: "tznc/treq" });
    renderOpenIn(document, { kind: "repo", repo: "tznc/treq" });
    expect(document.querySelectorAll("#treq-open-in")).toHaveLength(1);
    renderOpenIn(document, { kind: "repo", repo: "tznc/s2b" });
    expect(document.querySelectorAll("#treq-open-in")).toHaveLength(1);
    expect(links()[2][1]).toBe("treq://repo/open?repo=tznc%2Fs2b");
  });

  it("removes the links on pages without them", () => {
    renderOpenIn(document, { kind: "repo", repo: "tznc/treq" });
    renderOpenIn(document, { kind: "other", repo: "tznc/treq" });
    expect(document.querySelector("#treq-open-in")).toBeNull();
  });
});
