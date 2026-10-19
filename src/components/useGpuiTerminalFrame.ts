import { type RefObject, useLayoutEffect, useState } from "react";

/** Room for the controls' shadow (`shadow-sm`). */
const SHADOW = 2;

/**
 * The highest edge the pane draws: its own, or its controls', which sit
 * above it (`-top-7`, or the collapsed pane's expand button).
 */
function paneTop(pane: HTMLElement): number {
  const controls = pane.querySelectorAll<HTMLElement>(
    '[data-testid="terminal-pane-controls"], [data-testid="terminal-pane-controls"] *',
  );
  let top = pane.getBoundingClientRect().top;
  for (const el of controls) {
    const rect = el.getBoundingClientRect();
    if (rect.width > 0 && rect.height > 0) top = Math.min(top, rect.top);
  }
  return top;
}

/**
 * While GPUI draws the Code tab (`contentHeight` set), the terminal pane's
 * layer spans that height but the webview shows only its bottom, framed
 * below GPUI's tab. Returns how tall that frame must be to show the pane
 * whole, measured from the pane in `layer`.
 */
export function useGpuiTerminalFrame(
  layer: RefObject<HTMLElement | null>,
  contentHeight: number | null,
): number | null {
  const [frameHeight, setFrameHeight] = useState<number | null>(null);

  useLayoutEffect(() => {
    const root = layer.current;
    if (contentHeight === null || !root) {
      setFrameHeight(null);
      return;
    }
    const measure = () => {
      const pane = root.querySelector<HTMLElement>(
        '[data-testid="workspace-terminal-pane"]',
      );
      if (!pane) return;
      const bottom = root.getBoundingClientRect().bottom;
      const height = bottom - paneTop(pane) + SHADOW;
      setFrameHeight(Math.round(Math.min(contentHeight, height)));
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(root);
    const pane = root.querySelector('[data-testid="workspace-terminal-pane"]');
    if (pane) observer.observe(pane);
    const mutations = new MutationObserver(measure);
    mutations.observe(root, {
      attributes: true,
      attributeFilter: ["data-collapsed", "style"],
      subtree: true,
    });
    return () => {
      observer.disconnect();
      mutations.disconnect();
    };
  }, [layer, contentHeight]);

  return frameHeight;
}
