export interface ElementComment {
  id: string;
  selector: string;
  tag: string;
  textPreview: string;
  htmlSnippet: string;
  x: number;
  y: number;
  width: number;
  height: number;
  text: string;
  createdAt: string;
}

export interface PickedElement {
  selector: string;
  tag: string;
  textPreview: string;
  htmlSnippet: string;
  x: number;
  y: number;
  width: number;
  height: number;
}

/** An externally requested navigation, e.g. from `treq send --browser`. */
export interface BrowserOpenRequest {
  id: string;
  url: string;
}

export interface BrowserPanelProps {
  repoPath?: string;
  workspaceId?: number;
  onCreateAgentWithReview?: (
    reviewMarkdown: string,
    mode: "plan" | "acceptEdits",
    useIdleAgent?: boolean,
  ) => Promise<void>;
  onReviewSubmitted?: () => void;
  openRequest?: BrowserOpenRequest | null;
  /**
   * DOM node the address bar (URL input + reload + select toggle) should be
   * portaled into, e.g. the workspace tab row, so it sits alongside the
   * Code/Commits/Changes tabs instead of its own row. Falls back to
   * rendering inline when absent.
   */
  toolbarSlot?: HTMLElement | null;
}
