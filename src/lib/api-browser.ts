import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { repoStateScope } from "./repo-state-scope";

// Split out of api-extra.ts/api-types.ts to keep those under the
// max-lines lint limit -- mirrors the api-github.ts precedent.

export interface ElementComment {
  id: string;
  selector: string;
  tag: string;
  text_preview: string;
  html_snippet: string;
  x: number;
  y: number;
  width: number;
  height: number;
  text: string;
  created_at: string;
}

export interface PendingPageReview {
  id: number;
  workspace_id: number;
  url: string;
  comments: ElementComment[];
  summary_text: string | null;
  created_at: string;
  updated_at: string;
}

export interface PickedElement {
  selector: string;
  tag: string;
  text_preview: string;
  html_snippet: string;
  x: number;
  y: number;
  width: number;
  height: number;
}

// Pending page review API (in-app browser)
export const loadPendingPageReview = (
  repoPath: string,
  workspaceId: number,
): Promise<PendingPageReview | null> =>
  invoke("load_pending_page_review", {
    repoPath: repoStateScope(repoPath),
    workspaceId,
  }).then((review) => {
    if (!review) return null;
    const normalized = { ...review } as PendingPageReview & {
      comments: unknown;
    };
    if (typeof normalized.comments === "string") {
      normalized.comments = JSON.parse(normalized.comments);
    }
    return normalized as PendingPageReview;
  });

export const savePendingPageReview = (
  repoPath: string,
  workspaceId: number,
  url: string,
  comments: ElementComment[],
  summaryText?: string,
): Promise<number> =>
  invoke("save_pending_page_review", {
    repoPath: repoStateScope(repoPath),
    workspaceId,
    url,
    comments: JSON.stringify(comments),
    summaryText: summaryText ?? null,
  });

export const clearPendingPageReview = (
  repoPath: string,
  workspaceId: number,
): Promise<void> =>
  invoke("clear_pending_page_review", {
    repoPath: repoStateScope(repoPath),
    workspaceId,
  });

// In-app browser webview control API
export const openBrowserWebview = (
  url: string,
  x: number,
  y: number,
  width: number,
  height: number,
): Promise<void> =>
  invoke("open_browser_webview", { url, x, y, width, height });

export const navigateBrowserWebview = (url: string): Promise<void> =>
  invoke("navigate_browser_webview", { url });

export const closeBrowserWebview = (): Promise<void> =>
  invoke("close_browser_webview");

export const setBrowserSelectMode = (enabled: boolean): Promise<void> =>
  invoke("set_browser_select_mode", { enabled });

export const syncBrowserWebviewBounds = (
  x: number,
  y: number,
  width: number,
  height: number,
): Promise<void> =>
  invoke("sync_browser_webview_bounds", { x, y, width, height });

export const listenBrowserElementPicked = (
  callback: (element: PickedElement) => void,
) =>
  listen<PickedElement>("browser-element-picked", (event) =>
    callback(event.payload),
  );

export const listenBrowserUrlChanged = (callback: (url: string) => void) =>
  listen<string>("browser-url-changed", (event) => callback(event.payload));
