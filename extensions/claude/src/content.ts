import { decorateMenus, menuOwner, type Owner } from "./menus";
import { applySessionTitle } from "./session";

// Menus open in a portal: remember the row whose button opened one.
let owner: Owner = { session: null, pr: null };
const remember = (e: Event) => {
  if (e.target instanceof Element && !e.target.closest('[role="menu"]'))
    owner = menuOwner(e.target);
};
document.addEventListener("pointerdown", remember, true);
document.addEventListener(
  "keydown",
  (e) => ["Enter", " ", "ArrowDown"].includes(e.key) && remember(e),
  true,
);

// Claude navigates without reloading and sets its own title: re-apply on
// any DOM or title change, at most once a frame.
let queued = false;
function render(): void {
  queued = false;
  applySessionTitle(document, location.pathname);
  decorateMenus(document, location.pathname, owner, (url) => {
    location.href = url;
  });
}

new MutationObserver(() => {
  if (!queued) {
    queued = true;
    requestAnimationFrame(render);
  }
}).observe(document.documentElement, {
  childList: true,
  subtree: true,
  characterData: true,
});
render();
