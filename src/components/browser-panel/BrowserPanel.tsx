import { isTauri } from "@tauri-apps/api/core";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { v4 as uuidv4 } from "uuid";
import {
  closeBrowserWebview,
  listenBrowserUrlChanged,
  navigateBrowserWebview,
  openBrowserWebview,
  syncBrowserWebviewBounds,
} from "../../lib/api";
import { useToast } from "../ui/toast";
import { CommentInput } from "../CommentInput";
import { AddressBar } from "./AddressBar";
import { BrowserReviewActionBar } from "./BrowserReviewActionBar";
import { useBrowserElementPicker } from "./hooks/useBrowserElementPicker";
import { useBrowserReview } from "./hooks/useBrowserReview";
import type { BrowserPanelProps, ElementComment } from "./types";

const DEFAULT_URL = "http://localhost:3000";

export function BrowserPanel({
  repoPath,
  workspaceId,
  onCreateAgentWithReview,
  onReviewSubmitted,
  openRequest,
  toolbarSlot,
}: BrowserPanelProps) {
  const { addToast } = useToast();
  const [url, setUrl] = useState(DEFAULT_URL);
  const [webviewOpen, setWebviewOpen] = useState(false);
  const previewRef = useRef<HTMLDivElement | null>(null);
  // Outside a real Tauri window (dev preview in a plain browser, or the
  // screenshot/test harness) there's no native child webview to create --
  // render an <iframe> instead so the preview is still visible. The real
  // packaged app always runs inside Tauri and keeps using the native
  // webview, which is what makes click-to-select possible.
  const runningInTauri = useMemo(() => isTauri(), []);

  const {
    comments,
    summary,
    setSummary,
    sendingReview,
    addComment,
    deleteComment,
    handleRequestChanges,
    handleCancelReview,
  } = useBrowserReview({
    url,
    repoPath,
    workspaceId,
    onCreateAgentWithReview,
    onReviewSubmitted,
    addToast,
  });

  const { selectMode, pendingPick, toggleSelectMode, clearPendingPick } =
    useBrowserElementPicker(webviewOpen);

  const [popoverOpen, setPopoverOpen] = useState(false);
  const [showCancelDialog, setShowCancelDialog] = useState(false);

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

  // External navigation requests, e.g. from `treq send --browser`.
  useEffect(() => {
    if (!openRequest) return;
    handleNavigate(openRequest.url);
  }, [openRequest?.id, handleNavigate]);

  const handleSubmitComment = useCallback(
    (text: string) => {
      if (!pendingPick) return;
      const comment: ElementComment = {
        id: uuidv4(),
        selector: pendingPick.selector,
        tag: pendingPick.tag,
        textPreview: pendingPick.textPreview,
        htmlSnippet: pendingPick.htmlSnippet,
        x: pendingPick.x,
        y: pendingPick.y,
        width: pendingPick.width,
        height: pendingPick.height,
        text,
        createdAt: new Date().toISOString(),
      };
      addComment(comment);
      clearPendingPick();
    },
    [pendingPick, addComment, clearPendingPick],
  );

  const addressBar = (
    <AddressBar
      url={url}
      onNavigate={handleNavigate}
      onReload={handleReload}
      selectMode={selectMode}
      onToggleSelectMode={toggleSelectMode}
    />
  );

  return (
    <div className="h-full flex flex-col">
      {toolbarSlot ? createPortal(addressBar, toolbarSlot) : addressBar}
      <div className="flex-1 min-h-0 relative" ref={previewRef}>
        {!webviewOpen && (
          <div className="h-full flex items-center justify-center text-sm text-muted-foreground">
            Enter a localhost URL or a local HTML file to start reviewing.
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
      {pendingPick && (
        <div className="border-t px-2 py-1">
          <div className="px-2 pt-1 text-xs text-muted-foreground font-mono truncate">
            {pendingPick.selector} ({pendingPick.tag})
          </div>
          <CommentInput
            onSubmit={handleSubmitComment}
            onCancel={clearPendingPick}
          />
        </div>
      )}
      <BrowserReviewActionBar
        comments={comments}
        summary={summary}
        setSummary={setSummary}
        sendingReview={sendingReview}
        popoverOpen={popoverOpen}
        setPopoverOpen={setPopoverOpen}
        showCancelDialog={showCancelDialog}
        setShowCancelDialog={setShowCancelDialog}
        onDeleteComment={deleteComment}
        onCancelReview={() => {
          handleCancelReview();
          setShowCancelDialog(false);
        }}
        onRequestChanges={(mode, useIdleAgent) => {
          handleRequestChanges(mode, useIdleAgent);
          setPopoverOpen(false);
        }}
      />
    </div>
  );
}
