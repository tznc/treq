import { type RefObject, useLayoutEffect, useState } from "react";

/**
 * Room above an expanded pane's top edge: its controls sit 28px above it
 * (`-top-7`), over the resize handle.
 */
const EXPANDED_OVERHANG = 28;
/** A collapsed pane's bar keeps a 6px margin (`m-1.5`). */
const COLLAPSED_MARGIN = 6;

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
      const collapsed = pane.dataset.collapsed === "true";
      const above = collapsed ? COLLAPSED_MARGIN : EXPANDED_OVERHANG;
      const bottom = root.getBoundingClientRect().bottom;
      const height = bottom - pane.getBoundingClientRect().top + above;
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
