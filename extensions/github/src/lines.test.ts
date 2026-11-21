import { beforeEach, describe, expect, it } from "vitest";
import { blobFile, blobLine, diffLine, prHeadRef } from "./lines";

beforeEach(() => {
  document.body.innerHTML = "";
});

describe("diffLine", () => {
  const diff = `<div data-tagsearch-path="src/a.rs"><table><tbody>
    <tr id="ctx"><td data-line-number="9"></td><td data-line-number="10"></td><td class="code">x</td></tr>
    <tr id="del"><td data-line-number="11"></td><td></td><td class="code">-y</td></tr>
  </tbody></table></div>`;

  it("names the file and new-side line of a hovered diff row", () => {
    document.body.innerHTML = diff;
    expect(diffLine(document.querySelector("#ctx .code")!)).toEqual({ path: "src/a.rs", line: 10 });
  });

  it("falls back to the old side for a deleted line", () => {
    document.body.innerHTML = diff;
    expect(diffLine(document.querySelector("#del .code")!)).toEqual({ path: "src/a.rs", line: 11 });
  });

  it("ignores anything outside a diff", () => {
    document.body.innerHTML = `<p id="p">hi</p>`;
    expect(diffLine(document.querySelector("#p")!)).toBeNull();
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
