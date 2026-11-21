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
