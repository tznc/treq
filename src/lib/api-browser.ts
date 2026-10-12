import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

// Split out of api-extra.ts/api-types.ts to keep those under the
// max-lines lint limit -- mirrors the api-github.ts precedent.

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

export const syncBrowserWebviewBounds = (
  x: number,
  y: number,
  width: number,
  height: number,
): Promise<void> =>
  invoke("sync_browser_webview_bounds", { x, y, width, height });

export const listenBrowserUrlChanged = (callback: (url: string) => void) =>
  listen<string>("browser-url-changed", (event) => callback(event.payload));
