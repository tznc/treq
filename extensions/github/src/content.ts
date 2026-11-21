import { loadSettings, providers, type Store } from "./settings";
import { LineComposer } from "./composer";
import { blobContext, blobFile, blobLine, diffContext, prHeadRef } from "./lines";
import type { CodeAt } from "./prompt";
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

/** The code under the pointer (a line, or the selection holding it), and where it is drawn. */
function codeUnder(target: Element, x: number, y: number): [CodeAt, DOMRect] | [null, null] {
  const page = currentPage(document, location.pathname);
  if (!page) return [null, null];
  const diff = page.kind === "pr" ? diffContext(target) : null;
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
  const lines = blobContext(document, Number(number.dataset.lineNumber), location.hash);
  return [{ repo: page.repo, ...file, lines }, new DOMRect(box.left, line.top, box.width, line.height)];
}

const composer = new LineComposer(document);
let pointer: MouseEvent | null = null;
document.addEventListener(
  "mousemove",
  (event) => {
    if (!pointer)
      requestAnimationFrame(() => {
        const e = pointer!;
        pointer = null;
        if (!(e.target instanceof Element) || composer.root.contains(e.target)) return;
        composer.hover(...codeUnder(e.target, e.clientX, e.clientY));
      });
    pointer = event;
  },
  { passive: true },
);

// Which agents the line buttons offer, from the options page.
const store = chrome.storage.sync as Store;
const applySettings = () =>
  loadSettings(store).then((settings) => {
    composer.providers = providers(settings);
  });
applySettings();
chrome.storage.onChanged.addListener((_, area) => area === "sync" && applySettings());
