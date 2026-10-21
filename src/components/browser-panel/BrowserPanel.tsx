import { isTauri } from "@tauri-apps/api/core";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import {
  closeBrowserWebview,
  listenBrowserUrlChanged,
  navigateBrowserWebview,
  openBrowserWebview,
  syncBrowserWebviewBounds,
} from "../../lib/api";
import { useGpuiPart } from "../../lib/gpui-parts";
import { useToast } from "../ui/toast";
import { AddressBar } from "./AddressBar";
import type { BrowserPanelProps } from "./types";
import { DEFAULT_BROWSER_URL } from "./utils";

export function BrowserPanel({ openRequest, toolbarSlot }: BrowserPanelProps) {
  const { addToast } = useToast();
  const [url, setUrl] = useState(DEFAULT_BROWSER_URL);
  const [webviewOpen, setWebviewOpen] = useState(false);
  const previewRef = useRef<HTMLDivElement | null>(null);
  // Outside a real Tauri window (dev preview in a plain browser, or the
  // screenshot/test harness) there's no native child webview to create --
  // render an <iframe> instead so the preview is still visible. The real
  // packaged app always runs inside Tauri and keeps using the native
  // webview, which is what makes click-to-select possible.
  const runningInTauri = useMemo(() => isTauri(), []);

  const syncBounds = useCallback(() => {
    if (!runningInTauri) return;
    const el = previewRef.current;
    if (!el) return;
    const rect = el.getBoundingClientRect();
    syncBrowserWebviewBounds(rect.x, rect.y, rect.width, rect.height).catch(
      () => {
        // best-effort: webview may not be open yet
      },
    );
  }, [runningInTauri]);

  useEffect(() => {
    const el = previewRef.current;
    if (!el) return;
    const observer = new ResizeObserver(() => syncBounds());
    observer.observe(el);
    return () => observer.disconnect();
  }, [syncBounds]);

  useEffect(
    () => () => {
      if (runningInTauri) {
        closeBrowserWebview().catch(() => {});
      }
    },
    [runningInTauri],
  );

  // Keep the address bar in sync when the user navigates within the
  // preview itself (clicking a link, form submission, etc.).
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    listenBrowserUrlChanged((nextUrl) => setUrl(nextUrl)).then((fn) => {
      if (cancelled) {
        fn();
        return;
      }
      unlisten = fn;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  const handleNavigate = useCallback(
    async (nextUrl: string) => {
      if (!runningInTauri) {
        setUrl(nextUrl);
        setWebviewOpen(true);
        return;
      }
      const el = previewRef.current;
      const rect = el?.getBoundingClientRect();
      try {
        if (webviewOpen) {
          await navigateBrowserWebview(nextUrl);
        } else {
          await openBrowserWebview(
            nextUrl,
            rect?.x ?? 0,
            rect?.y ?? 0,
            rect?.width ?? 0,
            rect?.height ?? 0,
          );
          setWebviewOpen(true);
        }
        setUrl(nextUrl);
      } catch (error) {
        addToast({
          title: "Failed to load page",
          description: error instanceof Error ? error.message : String(error),
          type: "error",
        });
      }
    },
    [webviewOpen, addToast, runningInTauri],
  );

  const handleReload = useCallback(() => {
    handleNavigate(url);
  }, [handleNavigate, url]);

  // GPUI draws the address bar in the tab row: it shows this url, and asks
  // for the navigations and reloads this panel runs.
  useGpuiPart(
    "browser",
    { url },
    { navigate: handleNavigate, reload: handleReload },
  );

  // External navigation requests, e.g. from `treq send --browser`.
  useEffect(() => {
    if (!openRequest) return;
    handleNavigate(openRequest.url);
  }, [openRequest?.id, handleNavigate]);

  const addressBar = (
    <AddressBar url={url} onNavigate={handleNavigate} onReload={handleReload} />
  );

  return (
    <div className="h-full flex flex-col">
      {toolbarSlot ? createPortal(addressBar, toolbarSlot) : addressBar}
      <div className="flex-1 min-h-0 relative" ref={previewRef}>
        {!webviewOpen && (
          <div className="h-full flex items-center justify-center text-sm text-muted-foreground">
            Enter a localhost URL or a local HTML file to preview it.
          </div>
        )}
        {webviewOpen && !runningInTauri && (
          <iframe
            src={url}
            title="Browser preview"
            className="w-full h-full border-0"
          />
        )}
      </div>
    </div>
  );
}
