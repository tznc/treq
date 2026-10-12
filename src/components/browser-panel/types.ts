/** An externally requested navigation, e.g. from `treq send --browser`. */
export interface BrowserOpenRequest {
  id: string;
  url: string;
}

export interface BrowserPanelProps {
  openRequest?: BrowserOpenRequest | null;
  /**
   * DOM node the address bar (URL input + reload + select toggle) should be
   * portaled into, e.g. the workspace tab row, so it sits alongside the
   * Code/Commits/Changes tabs instead of its own row. Falls back to
   * rendering inline when absent.
   */
  toolbarSlot?: HTMLElement | null;
}
