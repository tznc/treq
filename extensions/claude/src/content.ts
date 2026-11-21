import { applySessionTitle } from "./session";

// Claude navigates without reloading and sets its own title: re-apply on
// any DOM or title change, at most once a frame.
let queued = false;
function render(): void {
  queued = false;
  applySessionTitle(document, location.pathname);
}

new MutationObserver(() => {
  if (!queued) {
    queued = true;
    requestAnimationFrame(render);
  }
}).observe(document.documentElement, { childList: true, subtree: true, characterData: true });
render();
