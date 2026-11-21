import { LinePrompt } from "./line-prompt";
import type { LineAt } from "./links";
import { blobFile, blobLine, diffLine, prHeadRef } from "./lines";
import { currentPage } from "./page";
import { renderOpenIn } from "./sidebar";

// GitHub navigates without reloading and re-renders often: redraw on any
// DOM change, at most once a frame. Each render is idempotent.
let queued = false;
function render(): void {
  queued = false;
  const page = currentPage(document, location.pathname);
  renderOpenIn(document, page ?? { kind: "other", repo: "" });
}

new MutationObserver(() => {
  if (!queued) {
    queued = true;
    requestAnimationFrame(render);
  }
}).observe(document.documentElement, { childList: true, subtree: true });
render();

/** The code line under the pointer, and where it is drawn. */
function lineUnder(target: Element, x: number, y: number): [LineAt, DOMRect] | [null, null] {
  const page = currentPage(document, location.pathname);
  if (!page) return [null, null];
  const diff = page.kind === "pr" ? diffLine(target) : null;
  const ref = diff && prHeadRef(document);
  const row = target.closest("tr");
  if (diff && ref && row) return [{ repo: page.repo, ref, ...diff }, row.getBoundingClientRect()];

  // Blob code is drawn under a textarea outside the code element: go by position.
  const box = document.querySelector(".react-code-file-contents")?.getBoundingClientRect();
  const inside = box && x >= box.left && x < box.right && y >= box.top && y < box.bottom;
  const file = inside && blobFile(document, location.pathname);
  const number = file && blobLine(document, y);
  if (!box || !file || !number) return [null, null];
  const line = number.getBoundingClientRect();
  return [
    { repo: page.repo, ...file, line: Number(number.dataset.lineNumber) },
    new DOMRect(box.left, line.top, box.width, line.height),
  ];
}

const prompt = new LinePrompt(document);
let pointer: MouseEvent | null = null;
document.addEventListener(
  "mousemove",
  (event) => {
    if (!pointer) requestAnimationFrame(() => {
      const e = pointer!;
      pointer = null;
      if (!(e.target instanceof Element) || prompt.root.contains(e.target)) return;
      prompt.hover(...lineUnder(e.target, e.clientX, e.clientY));
    });
    pointer = event;
  },
  { passive: true },
);
