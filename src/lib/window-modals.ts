import { useEffect, useState } from "react";

/**
 * Modals that cover the whole window. Under the GPUI shell the sidebar is
 * outside the webview, so GPUI draws their backdrop over it.
 */
const WINDOW_MODAL_SELECTOR = "[data-window-modal], [cmdk-dialog]";
const BACKDROP_SELECTOR = "[data-window-modal-backdrop], [cmdk-overlay]";

/** CSS variable: how far the window extends left of the webview. */
const OFFSET_VARIABLE = "--window-offset-left";

/** A backdrop's alpha, from `rgba(…, a)` or `oklab(… / a)`; 0.4 if unreadable. */
function backdropOpacity(backdrop: Element | undefined): number {
  const color = backdrop ? getComputedStyle(backdrop).backgroundColor : "";
  const match = /[,/]\s*([\d.]+)(%?)\s*\)\s*$/.exec(color);
  const value = match
    ? Number(match[1]) / (match[2] === "%" ? 100 : 1)
    : Number.NaN;
  return Number.isFinite(value) && value > 0 ? value : 0.4;
}

function topBackdrop(): HTMLElement | undefined {
  const backdrops = document.querySelectorAll<HTMLElement>(BACKDROP_SELECTOR);
  return backdrops[backdrops.length - 1];
}

/** The open window modal's backdrop opacity, or null with none open. */
export function windowModalBackdrop(): number | null {
  if (document.querySelector(WINDOW_MODAL_SELECTOR) === null) return null;
  return backdropOpacity(topBackdrop());
}

/** {@link windowModalBackdrop}, kept current while `enabled`. */
export function useWindowModalBackdrop(enabled: boolean): number | null {
  const [backdrop, setBackdrop] = useState<number | null>(null);
  useEffect(() => {
    if (!enabled) {
      setBackdrop(null);
      return;
    }
    const update = () => setBackdrop(windowModalBackdrop());
    update();
    const observer = new MutationObserver(update);
    observer.observe(document.body, { childList: true, subtree: true });
    return () => observer.disconnect();
  }, [enabled]);
  return backdrop;
}

/** Closes the topmost window modal as a click on its own backdrop would. */
export function dismissTopWindowModal(): void {
  const backdrop = topBackdrop();
  if (!backdrop) return;
  backdrop.dispatchEvent(
    new PointerEvent("pointerdown", { bubbles: true, cancelable: true }),
  );
  backdrop.click();
}

/** Centres modals in the window, `offset` px of which lies left of the webview. */
export function setWindowOffsetLeft(offset: number | null): void {
  const { style } = document.documentElement;
  if (offset === null) style.removeProperty(OFFSET_VARIABLE);
  else style.setProperty(OFFSET_VARIABLE, `${offset}px`);
}
