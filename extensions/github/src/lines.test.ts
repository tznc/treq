import { beforeEach, describe, expect, it } from "vitest";
import { blobContext, blobFile, blobLine, diffContext, prHeadRef } from "./lines";

beforeEach(() => {
  document.body.innerHTML = "";
});

describe("diffContext", () => {
  const row = (id: string, left: string, right: string, code: string, selected = false) =>
    `<tr id="${id}"><td ${left ? `data-line-number="${left}"` : ""} class="${selected ? "selected-line" : ""}"></td>
      <td ${right ? `data-line-number="${right}"` : ""}></td>
      <td class="blob-code"><span class="blob-code-inner" data-code-marker="${left && right ? " " : left ? "-" : "+"}">${code}</span></td></tr>`;
  const diff = (rows: string) =>
    `<div data-tagsearch-path="src/a.rs"><table><tbody>${rows}</tbody></table></div>`;

  it("takes the hovered row's new-side line and code", () => {
    document.body.innerHTML = diff(row("a", "9", "10", "let x = 1;"));
    expect(diffContext(document.querySelector("#a .blob-code")!)).toEqual({
      path: "src/a.rs",
      lines: [{ line: 10, text: " let x = 1;" }],
    });
  });

  it("takes the old side of a deleted line, marking each line as the diff does", () => {
    document.body.innerHTML = diff(row("a", "11", "", "gone"));
    expect(diffContext(document.querySelector("#a")!)?.lines).toEqual([{ line: 11, text: "-gone" }]);
  });

  it("takes every selected row when the hovered one is selected", () => {
    document.body.innerHTML = diff(
      row("a", "1", "1", "a", true) + row("b", "2", "", "b", true) + row("c", "", "2", "c", true) + row("d", "3", "3", "d"),
    );
    expect(diffContext(document.querySelector("#b")!)?.lines.map((l) => l.text)).toEqual([" a", "-b", "+c"]);
    expect(diffContext(document.querySelector("#d")!)?.lines.map((l) => l.text)).toEqual([" d"]);
  });

  it("ignores anything outside a diff", () => {
    document.body.innerHTML = `<p id="p">hi</p>`;
    expect(diffContext(document.querySelector("#p")!)).toBeNull();
  });
});

describe("blobContext", () => {
  beforeEach(() => {
    document.body.innerHTML = `<textarea id="read-only-cursor-text-area">one\ntwo\nthree\nfour</textarea>`;
  });

  it("takes the hovered line", () => {
    expect(blobContext(document, 2, "")).toEqual([{ line: 2, text: "two" }]);
  });

  it("takes the selected lines when the hovered one is among them", () => {
    expect(blobContext(document, 3, "#L2-L4").map((l) => l.line)).toEqual([2, 3, 4]);
    expect(blobContext(document, 1, "#L2-L4").map((l) => l.line)).toEqual([1]);
    expect(blobContext(document, 2, "#L2").map((l) => l.line)).toEqual([2]);
  });

  it("reads drawn lines without the file text", () => {
    document.body.innerHTML = `<div id="LC7">seven</div>`;
    expect(blobContext(document, 7, "")).toEqual([{ line: 7, text: "seven" }]);
  });
});

describe("blobLine", () => {
  it("finds the line number drawn at a height", () => {
    document.body.innerHTML = `<div class="react-code-file-contents">
      <div class="react-line-number" data-line-number="1"></div>
      <div class="react-line-number" data-line-number="2"></div></div>`;
    document.querySelectorAll<HTMLElement>(".react-line-number").forEach((el, i) => {
      el.getBoundingClientRect = () => ({ top: i * 20, bottom: i * 20 + 20 }) as DOMRect;
    });
    expect(blobLine(document, 25)?.dataset.lineNumber).toBe("2");
    expect(blobLine(document, 99)).toBeNull();
  });
});

describe("blobFile", () => {
  it("splits a slashed branch from the path with GitHub's page data", () => {
    document.body.innerHTML = `<script type="application/json" data-target="react-app.embeddedData">
      {"payload":{"codeViewLayoutRoute":{"path":"src/a.rs","refInfo":{"name":"claude/x"}}}}</script>`;
    expect(blobFile(document, "/tznc/treq/blob/claude/x/src/a.rs")).toEqual({ ref: "claude/x", path: "src/a.rs" });
  });

  it("reads the first segment as the ref without page data for this file", () => {
    expect(blobFile(document, "/tznc/treq/blob/main/src/a.rs")).toEqual({ ref: "main", path: "src/a.rs" });
    expect(blobFile(document, "/tznc/treq/tree/main")).toBeNull();
  });
});

describe("prHeadRef", () => {
  it("reads the head branch from a pull request's header", () => {
    document.body.innerHTML = `<span class="commit-ref css-truncate">main</span>
      <span class="commit-ref css-truncate"> claude/x </span>`;
    expect(prHeadRef(document)).toBe("claude/x");
  });
});
