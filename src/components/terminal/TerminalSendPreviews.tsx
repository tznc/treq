import { useEffect, useRef, useState } from "react";
import { FileText, X } from "lucide-react";
import { assetsForPtySession, treqSendFileSrc } from "../../lib/treqSend";
import {
  canAcceptTerminalDrop,
  copySendAssetToClipboard,
  setSendAssetDragData,
  terminalInsertTextFromDrop,
} from "../../lib/send-asset-drag";
import { cn } from "../../lib/utils";
import { useTreqSendStore } from "../../stores/treqSendStore";
import {
  Attachment,
  AttachmentAction,
  AttachmentActions,
  AttachmentGroup,
  AttachmentMedia,
  AttachmentTrigger,
} from "../ui/attachment";
import { SendAssetLightbox } from "./SendAssetLightbox";

interface TerminalSendPreviewsProps {
  ptySessionId: string;
  isActive?: boolean;
  className?: string;
  onSendReview?: (prompt: string) => void;
  onInsertIntoTerminal?: (text: string) => void;
}

export function TerminalSendPreviews({
  ptySessionId,
  isActive = false,
  className,
  onSendReview,
  onInsertIntoTerminal,
}: TerminalSendPreviewsProps) {
  const sendAssets = useTreqSendStore((s) => s.assets);
  const dismissAsset = useTreqSendStore((s) => s.dismissAsset);
  const [previewIndex, setPreviewIndex] = useState<number | null>(null);
  const [hoveredId, setHoveredId] = useState<string | null>(null);
  const skipClickAfterDragRef = useRef(false);

  const assets = assetsForPtySession(sendAssets, ptySessionId, isActive).filter(
    (asset) => asset.mediaType !== "browser",
  );

  const hoveredAsset = hoveredId
    ? assets.find((item) => item.id === hoveredId)
    : undefined;

  useEffect(() => {
    if (!hoveredAsset) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (!(event.metaKey || event.ctrlKey) || event.key !== "c") return;
      const target = event.target as HTMLElement | null;
      if (
        target?.tagName === "INPUT" ||
        target?.tagName === "TEXTAREA" ||
        target?.getAttribute("contenteditable") === "true"
      ) {
        return;
      }
      event.preventDefault();
      copySendAssetToClipboard({
        path: hoveredAsset.path,
        title: hoveredAsset.title,
      }).catch(console.error);
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [hoveredAsset]);

  if (assets.length === 0 && previewIndex == null) return null;

  const openPreview = (assetId: string) => {
    const index = assets.findIndex((asset) => asset.id === assetId);
    if (index >= 0) setPreviewIndex(index);
  };

  return (
    <>
      {assets.length > 0 && (
        <div
          data-testid="terminal-send-previews"
          className={cn(
            "pointer-events-none absolute inset-x-0 top-0 z-10",
            className,
          )}
        >
          <div
            aria-hidden
            className="pointer-events-none absolute inset-0 bg-gradient-to-b from-[#1e1e1e] via-[#1e1e1e]/85 to-transparent"
          />
          <div
            data-testid="terminal-send-previews-drop"
            className="pointer-events-auto relative px-3 pb-8 pt-3"
            onDragOver={(e) => {
              if (!canAcceptTerminalDrop(e.dataTransfer)) return;
              e.preventDefault();
              e.dataTransfer.dropEffect = "copy";
            }}
            onDrop={(e) => {
              const text = terminalInsertTextFromDrop(e.dataTransfer);
              if (!text || !onInsertIntoTerminal) return;
              e.preventDefault();
              onInsertIntoTerminal(text);
            }}
          >
            <AttachmentGroup className="gap-2 overflow-x-auto py-0 pt-2 pr-2">
              {assets.map((asset) => (
                <Attachment
                  key={asset.id}
                  state="done"
                  variant="thumbnail"
                  draggable
                  className="cursor-grab active:cursor-grabbing"
                  onMouseEnter={() => setHoveredId(asset.id)}
                  onMouseLeave={() => setHoveredId(null)}
                  onDragStart={(event) => {
                    skipClickAfterDragRef.current = true;
                    setSendAssetDragData(event.dataTransfer, {
                      path: asset.path,
                      title: asset.title,
                    });
                  }}
                  onDragEnd={() => {
                    window.setTimeout(() => {
                      skipClickAfterDragRef.current = false;
                    }, 0);
                  }}
                  onCopy={(event) => {
                    if (!event.clipboardData) return;
                    event.preventDefault();
                    setSendAssetDragData(event.clipboardData, {
                      path: asset.path,
                      title: asset.title,
                    });
                  }}
                >
                  <AttachmentTrigger
                    data-testid={`terminal-send-preview-${asset.id}`}
                    aria-label={`Preview ${asset.title}`}
                    draggable
                    onDragStart={(event) => {
                      skipClickAfterDragRef.current = true;
                      setSendAssetDragData(event.dataTransfer, {
                        path: asset.path,
                        title: asset.title,
                      });
                    }}
                    onClick={() => {
                      if (skipClickAfterDragRef.current) return;
                      openPreview(asset.id);
                    }}
                  />
                  <AttachmentMedia
                    variant={asset.mediaType === "image" ? "image" : "icon"}
                  >
                    {asset.mediaType === "image" ? (
                      <img
                        src={treqSendFileSrc(asset.path)}
                        alt={asset.title}
                        className="h-full w-full object-contain"
                        draggable={false}
                      />
                    ) : (
                      <>
                        <FileText />
                        <span className="w-full truncate text-center text-[10px] leading-tight">
                          {asset.title}
                        </span>
                      </>
                    )}
                  </AttachmentMedia>
                  <AttachmentActions
                    className={cn(
                      "transition-opacity",
                      hoveredId === asset.id
                        ? "pointer-events-auto opacity-100"
                        : "pointer-events-none opacity-0",
                    )}
                  >
                    <AttachmentAction
                      type="button"
                      data-testid={`terminal-send-dismiss-${asset.id}`}
                      aria-label={`Dismiss ${asset.title}`}
                      className="h-5 w-5 rounded-full border border-zinc-500 bg-[#c4c4c4] text-zinc-900 shadow-md hover:bg-[#d4d4d4]"
                      onClick={() => {
                        dismissAsset(asset.id);
                        setHoveredId(null);
                        setPreviewIndex((current) => {
                          if (current == null) return null;
                          const remaining = assets.filter(
                            (a) => a.id !== asset.id,
                          );
                          if (remaining.length === 0) return null;
                          return Math.min(current, remaining.length - 1);
                        });
                      }}
                    >
                      <X className="h-3 w-3" strokeWidth={2.5} />
                    </AttachmentAction>
                  </AttachmentActions>
                </Attachment>
              ))}
            </AttachmentGroup>
          </div>
        </div>
      )}

      {previewIndex != null && assets.length > 0 && (
        <SendAssetLightbox
          assets={assets}
          initialIndex={Math.min(previewIndex, assets.length - 1)}
          onClose={() => setPreviewIndex(null)}
          onSendReview={onSendReview}
        />
      )}
    </>
  );
}
