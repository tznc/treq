import {
  useEffect,
  useRef,
  useState,
  type CSSProperties,
  type FormEvent,
  type KeyboardEvent as ReactKeyboardEvent,
} from "react";
import useSWR from "swr";
import { Copy, FolderOpen, X, ZoomIn, ZoomOut } from "lucide-react";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { readFile, writeSendReviewImage } from "../../lib/api";
import { type TreqSendAsset, treqSendFileSrc } from "../../lib/treqSend";
import {
  type AssetLineComment,
  type ImageHighlightComment,
  blobToBase64,
  formatSendAssetReviewPrompt,
  hasUnsentSendAssetReview,
  renderHighlightedImageBlob,
} from "../../lib/sendAssetReview";
import { copySendAssetToClipboard } from "../../lib/send-asset-drag";
import { useToast } from "../ui/toast";
import { type CarouselApi } from "../ui/carousel";
import { Button } from "../ui/button";
import { Input } from "../ui/input";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "../ui/alert-dialog";
import {
  IMAGE_ZOOM_DEFAULT,
  IMAGE_ZOOM_MAX,
  IMAGE_ZOOM_MIN,
  IMAGE_ZOOM_STEP,
  SendAssetLightboxCarousel,
  clampImageZoom,
  fallbackImageHeight,
  revealInFileManagerLabel,
} from "./SendAssetLightboxCarousel";

/** True when the click landed outside every visible piece of the lightbox. */
function isLightboxBackdrop(target: EventTarget | null) {
  return (
    target instanceof Element && !target.closest("[data-lightbox-content]")
  );
}

interface SendAssetLightboxProps {
  assets: TreqSendAsset[];
  initialIndex: number;
  onClose: () => void;
  onSendReview?: (prompt: string) => void;
}

export function SendAssetLightbox({
  assets,
  initialIndex,
  onClose,
  onSendReview,
}: SendAssetLightboxProps) {
  const { addToast } = useToast();
  const [api, setApi] = useState<CarouselApi>();
  const [currentIndex, setCurrentIndex] = useState(initialIndex);
  const [imageZoom, setImageZoom] = useState(IMAGE_ZOOM_DEFAULT);
  const [baseSizeById, setBaseSizeById] = useState<
    Record<string, { width: number; height: number }>
  >({});
  const [generalComment, setGeneralComment] = useState("");
  const [highlightsById, setHighlightsById] = useState<
    Record<string, ImageHighlightComment[]>
  >({});
  const [activeHighlightId, setActiveHighlightId] = useState<string | null>(
    null,
  );
  const [lineCommentsById, setLineCommentsById] = useState<
    Record<string, AssetLineComment[]>
  >({});
  const [lineComposerOpen, setLineComposerOpen] = useState(false);
  const [discardOpen, setDiscardOpen] = useState(false);
  const [sending, setSending] = useState(false);
  const revealLabel = revealInFileManagerLabel();

  const current = assets[currentIndex] ?? assets[0];
  const showingImage = current?.mediaType === "image";
  const highlights = current ? (highlightsById[current.id] ?? []) : [];
  const lineComments = current ? (lineCommentsById[current.id] ?? []) : [];
  const hasUnsent =
    hasUnsentSendAssetReview({
      generalComment,
      highlights,
      lineComments,
    }) || lineComposerOpen;

  useEffect(() => {
    if (!api) return;
    setCurrentIndex(initialIndex);
    api.scrollTo(initialIndex, true);
    const onSelect = () => setCurrentIndex(api.selectedScrollSnap());
    api.on("select", onSelect);
    return () => {
      api.off("select", onSelect);
    };
  }, [api, initialIndex]);

  useEffect(() => {
    setImageZoom(IMAGE_ZOOM_DEFAULT);
    setActiveHighlightId(null);
  }, [currentIndex]);

  const rememberFittedBaseSize = (assetId: string, img: HTMLImageElement) => {
    if (img.naturalWidth <= 0 || img.naturalHeight <= 0) return;
    const maxH = window.innerHeight * 0.8;
    const maxW = Math.min(
      window.innerWidth * 0.9,
      Math.max(img.parentElement?.clientWidth || 0, window.innerWidth * 0.75),
    );
    const fitScale = Math.min(
      maxW / img.naturalWidth,
      maxH / img.naturalHeight,
    );
    const width = img.naturalWidth * fitScale;
    const height = img.naturalHeight * fitScale;
    setBaseSizeById((prev) => {
      const existing = prev[assetId];
      if (existing && existing.width === width && existing.height === height) {
        return prev;
      }
      return { ...prev, [assetId]: { width, height } };
    });
  };

  const imageSizeStyle = (assetId: string): CSSProperties => {
    const zoomFactor = assetId === current?.id ? imageZoom : IMAGE_ZOOM_DEFAULT;
    const base = baseSizeById[assetId];
    if (base) {
      return {
        width: base.width * zoomFactor,
        height: base.height * zoomFactor,
        maxWidth: "none",
        maxHeight: "none",
      };
    }
    return {
      height: fallbackImageHeight(zoomFactor),
      width: "auto",
      maxWidth: "none",
      maxHeight: "none",
    };
  };

  const mouseDownOnBackdropRef = useRef(false);

  const requestClose = () => {
    if (hasUnsent) {
      setDiscardOpen(true);
      return;
    }
    onClose();
  };

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        if (hasUnsent) {
          setDiscardOpen(true);
          return;
        }
        onClose();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [hasUnsent, onClose]);

  const textAssetKey = assets
    .filter((asset) => asset.mediaType === "text")
    .map((asset) => asset.path)
    .join("\0");
  const { data: textByPath = {} } = useSWR(
    textAssetKey ? ["send-asset-text", textAssetKey] : null,
    async () => {
      const textAssets = assets.filter((asset) => asset.mediaType === "text");
      const entries = await Promise.all(
        textAssets.map(async (asset) => {
          try {
            const content = await readFile(asset.path);
            return [asset.path, content] as const;
          } catch {
            return [asset.path, ""] as const;
          }
        }),
      );
      return Object.fromEntries(entries);
    },
  );

  const copyCurrentAsset = async () => {
    if (!current) return;
    try {
      await copySendAssetToClipboard({
        path: current.path,
        title: current.title,
      });
      addToast({
        title: "Copied",
        description: "Asset path copied to clipboard",
        type: "success",
      });
    } catch (error) {
      addToast({
        title: "Copy failed",
        description: error instanceof Error ? error.message : String(error),
        type: "error",
      });
    }
  };

  const revealCurrentAsset = async () => {
    if (!current) return;
    try {
      await revealItemInDir(current.path);
    } catch (error) {
      addToast({
        title: "Open failed",
        description: error instanceof Error ? error.message : String(error),
        type: "error",
      });
    }
  };

  const zoomIn = () => {
    setImageZoom((currentZoom) =>
      clampImageZoom(currentZoom + IMAGE_ZOOM_STEP),
    );
  };

  const zoomOut = () => {
    setImageZoom((currentZoom) =>
      clampImageZoom(currentZoom - IMAGE_ZOOM_STEP),
    );
  };

  const toggleImageZoom = () => {
    setImageZoom((currentZoom) =>
      currentZoom === IMAGE_ZOOM_DEFAULT ? IMAGE_ZOOM_MAX : IMAGE_ZOOM_DEFAULT,
    );
  };

  const sendReview = async () => {
    if (!current || !onSendReview) return;
    if (!hasUnsent) return;
    setSending(true);
    try {
      let annotatedImagePath: string | undefined;
      if (current.mediaType === "image" && highlights.length > 0) {
        try {
          const blob = await Promise.race([
            renderHighlightedImageBlob(
              treqSendFileSrc(current.path),
              highlights,
            ),
            new Promise<Blob>((_, reject) => {
              window.setTimeout(
                () => reject(new Error("Timed out drawing highlights")),
                2500,
              );
            }),
          ]);
          const contentsBase64 = await blobToBase64(blob);
          annotatedImagePath = await writeSendReviewImage(
            current.repo,
            `${current.title}-review.png`,
            contentsBase64,
          );
        } catch (error) {
          addToast({
            title: "Could not attach highlighted image",
            description: error instanceof Error ? error.message : String(error),
            type: "error",
          });
        }
      }
      onSendReview(
        formatSendAssetReviewPrompt({
          title: current.title,
          path: current.path,
          mediaType: current.mediaType,
          generalComment,
          highlights,
          lineComments,
          annotatedImagePath,
        }),
      );
      setGeneralComment("");
      setHighlightsById((prev) => ({ ...prev, [current.id]: [] }));
      setLineCommentsById((prev) => ({ ...prev, [current.id]: [] }));
      setActiveHighlightId(null);
      setLineComposerOpen(false);
    } finally {
      setSending(false);
    }
  };

  const onReviewKeyDown = (event: ReactKeyboardEvent<HTMLInputElement>) => {
    if (event.key === "Enter") {
      event.preventDefault();
      void sendReview();
    }
  };

  const onReviewSubmit = (event: FormEvent) => {
    event.preventDefault();
    void sendReview();
  };

  const lightboxControls = (
    <div className="flex max-w-full flex-wrap items-center justify-center gap-2">
      <div
        data-lightbox-content
        className="flex shrink-0 items-center gap-1 rounded-full border border-white/15 bg-black/40 p-1 shadow-lg backdrop-blur"
      >
        {showingImage && (
          <>
            <Button
              type="button"
              variant="ghost"
              size="icon-xs"
              className="h-8 w-8 text-white hover:bg-white/15 hover:text-white"
              aria-label="Zoom out"
              data-testid="treq-send-zoom-out"
              disabled={imageZoom <= IMAGE_ZOOM_MIN}
              onClick={zoomOut}
            >
              <ZoomOut className="h-4 w-4" />
            </Button>
            <span
              data-testid="treq-send-zoom-level"
              className="min-w-10 px-1 text-center text-xs tabular-nums text-white/80"
            >
              {Math.round(imageZoom * 100)}%
            </span>
            <Button
              type="button"
              variant="ghost"
              size="icon-xs"
              className="h-8 w-8 text-white hover:bg-white/15 hover:text-white"
              aria-label="Zoom in"
              data-testid="treq-send-zoom-in"
              disabled={imageZoom >= IMAGE_ZOOM_MAX}
              onClick={zoomIn}
            >
              <ZoomIn className="h-4 w-4" />
            </Button>
            <span aria-hidden className="mx-0.5 h-4 w-px bg-white/20" />
          </>
        )}
        <Button
          type="button"
          variant="ghost"
          size="icon-xs"
          className="h-8 w-8 text-white hover:bg-white/15 hover:text-white"
          aria-label="Copy asset"
          data-testid="treq-send-copy"
          onClick={copyCurrentAsset}
        >
          <Copy className="h-4 w-4" />
        </Button>
        <Button
          type="button"
          variant="ghost"
          size="icon-xs"
          className="h-8 w-8 text-white hover:bg-white/15 hover:text-white"
          aria-label={revealLabel}
          data-testid="treq-send-reveal"
          onClick={revealCurrentAsset}
        >
          <FolderOpen className="h-4 w-4" />
        </Button>
        <Button
          type="button"
          variant="ghost"
          size="icon-xs"
          className="h-8 w-8 text-white hover:bg-white/15 hover:text-white"
          aria-label="Close preview"
          data-testid="treq-send-close"
          onClick={requestClose}
        >
          <X className="h-4 w-4" />
        </Button>
      </div>
      <form
        data-lightbox-content
        className="min-w-56 max-w-md flex-1"
        onSubmit={onReviewSubmit}
      >
        <Input
          value={generalComment}
          onChange={(event) => setGeneralComment(event.target.value)}
          onKeyDown={onReviewKeyDown}
          placeholder="Add a review comment…"
          aria-label="Review comment"
          data-testid="treq-send-review-input"
          disabled={!onSendReview || sending}
          className="h-9 border-white/15 bg-black/40 text-sm text-white placeholder:text-white/50 focus-visible:ring-white/30"
        />
      </form>
    </div>
  );

  return (
    <div
      data-testid="treq-send-preview-lightbox"
      className="fixed inset-0 z-50 flex items-center justify-center"
    >
      <button
        type="button"
        aria-label="Close preview backdrop"
        className="absolute inset-0 cursor-default bg-black/55 backdrop-blur-md"
        onClick={requestClose}
      />
      <div
        data-testid="treq-send-preview-carousel-shell"
        className="relative z-10 min-w-[75vw] w-[90vw] px-3"
        onMouseDown={(event) => {
          mouseDownOnBackdropRef.current = isLightboxBackdrop(event.target);
        }}
        onClick={(event) => {
          // The shell covers most of the screen; treat clicks on its empty
          // space like clicks on the backdrop behind it.
          if (
            mouseDownOnBackdropRef.current &&
            isLightboxBackdrop(event.target)
          ) {
            requestClose();
          }
        }}
      >
        <div
          data-testid="treq-send-preview-header"
          className="mb-3 flex flex-col items-center gap-1.5"
        >
          {current && (
            <p
              data-lightbox-content
              className="max-w-full truncate text-center text-sm text-white/80"
            >
              {current.title}
            </p>
          )}
          {lightboxControls}
        </div>
        <SendAssetLightboxCarousel
          assets={assets}
          initialIndex={initialIndex}
          current={current}
          showingImage={showingImage}
          imageZoom={imageZoom}
          highlights={highlights}
          lineComments={lineComments}
          activeHighlightId={activeHighlightId}
          textByPath={textByPath}
          setApi={setApi}
          imageSizeStyle={imageSizeStyle}
          rememberFittedBaseSize={rememberFittedBaseSize}
          onActiveHighlightIdChange={setActiveHighlightId}
          onHighlightsChange={(next) => {
            if (!current) return;
            setHighlightsById((prev) => ({ ...prev, [current.id]: next }));
          }}
          onLineCommentsChange={(next) => {
            if (!current) return;
            setLineCommentsById((prev) => ({ ...prev, [current.id]: next }));
          }}
          onLineComposerChange={setLineComposerOpen}
          onToggleImageZoom={toggleImageZoom}
        />
      </div>
      <AlertDialog open={discardOpen} onOpenChange={setDiscardOpen}>
        <AlertDialogContent data-testid="treq-send-unsaved-dialog">
          <AlertDialogHeader>
            <AlertDialogTitle>Discard unsent comments?</AlertDialogTitle>
            <AlertDialogDescription>
              You have review comments that have not been sent to the agent.
              Close anyway and lose them?
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Keep reviewing</AlertDialogCancel>
            <AlertDialogAction
              data-testid="treq-send-unsaved-discard"
              onClick={onClose}
            >
              Discard comments
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}
