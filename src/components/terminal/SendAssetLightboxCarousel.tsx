import type { CSSProperties } from "react";
import { type TreqSendAsset, treqSendFileSrc } from "../../lib/treqSend";
import type {
  AssetLineComment,
  ImageHighlightComment,
} from "../../lib/sendAssetReview";
import { cn } from "../../lib/utils";
import {
  type CarouselApi,
  Carousel,
  CarouselContent,
  CarouselItem,
  CarouselNext,
  CarouselPrevious,
} from "../ui/carousel";
import { ImageHighlightLayer } from "./ImageHighlightLayer";
import { TextAssetLineReview } from "./TextAssetLineReview";

export const IMAGE_ZOOM_MIN = 1;
export const IMAGE_ZOOM_MAX = 2;
export const IMAGE_ZOOM_STEP = 1;
export const IMAGE_ZOOM_DEFAULT = 1;
/** Fallback height as vh when natural size is unknown (jsdom / slow decode). */
const IMAGE_ZOOM_FALLBACK_VIEWPORT_FRACTION = 0.8;

export function revealInFileManagerLabel(): string {
  const ua = navigator.userAgent.toLowerCase();
  if (ua.includes("mac")) return "Show in Finder";
  if (ua.includes("win")) return "Show in Explorer";
  return "Show in file manager";
}

export function clampImageZoom(value: number): number {
  return Math.min(
    IMAGE_ZOOM_MAX,
    Math.max(IMAGE_ZOOM_MIN, Math.round(value * 100) / 100),
  );
}

export function fallbackImageHeight(zoomFactor: number): string {
  const vh =
    Math.round(IMAGE_ZOOM_FALLBACK_VIEWPORT_FRACTION * zoomFactor * 10000) /
    100;
  return `${vh}vh`;
}

interface SendAssetLightboxCarouselProps {
  assets: TreqSendAsset[];
  initialIndex: number;
  current: TreqSendAsset | undefined;
  showingImage: boolean;
  imageZoom: number;
  highlights: ImageHighlightComment[];
  lineComments: AssetLineComment[];
  activeHighlightId: string | null;
  textByPath: Record<string, string>;
  setApi: (api: CarouselApi) => void;
  imageSizeStyle: (assetId: string) => CSSProperties;
  rememberFittedBaseSize: (assetId: string, img: HTMLImageElement) => void;
  onActiveHighlightIdChange: (id: string | null) => void;
  onHighlightsChange: (next: ImageHighlightComment[]) => void;
  onLineCommentsChange: (next: AssetLineComment[]) => void;
  onLineComposerChange: (open: boolean) => void;
  onToggleImageZoom: () => void;
}

export function SendAssetLightboxCarousel({
  assets,
  initialIndex,
  current,
  showingImage,
  imageZoom,
  highlights,
  lineComments,
  activeHighlightId,
  textByPath,
  setApi,
  imageSizeStyle,
  rememberFittedBaseSize,
  onActiveHighlightIdChange,
  onHighlightsChange,
  onLineCommentsChange,
  onLineComposerChange,
  onToggleImageZoom,
}: SendAssetLightboxCarouselProps) {
  return (
    <Carousel
      key={`send-carousel-${initialIndex}-${assets.map((a) => a.id).join(":")}`}
      setApi={setApi}
      opts={{ startIndex: initialIndex, loop: false }}
      className="w-full"
    >
      <CarouselContent>
        {assets.map((asset) => (
          <CarouselItem
            key={asset.id}
            className="flex min-h-0 items-stretch justify-center"
          >
            {asset.mediaType === "image" ? (
              <div
                data-testid={
                  asset.id === current?.id
                    ? "treq-send-image-scroll"
                    : undefined
                }
                data-lightbox-content={
                  asset.id === current?.id && imageZoom > IMAGE_ZOOM_DEFAULT
                    ? true
                    : undefined
                }
                className={cn(
                  "w-full max-w-full overflow-auto",
                  asset.id === current?.id && imageZoom > IMAGE_ZOOM_DEFAULT
                    ? "h-[80vh]"
                    : "max-h-[80vh]",
                )}
              >
                <div
                  className={cn(
                    "flex min-h-full min-w-full justify-center",
                    asset.id === current?.id && imageZoom > IMAGE_ZOOM_DEFAULT
                      ? "items-start"
                      : "items-center",
                  )}
                >
                  <div data-lightbox-content className="contents">
                    <ImageHighlightLayer
                      highlights={asset.id === current?.id ? highlights : []}
                      activeId={
                        asset.id === current?.id ? activeHighlightId : null
                      }
                      onActiveIdChange={onActiveHighlightIdChange}
                      onHighlightsChange={(next) => {
                        if (!current || asset.id !== current.id) return;
                        onHighlightsChange(next);
                      }}
                      onClickWithoutDrag={
                        asset.id === current?.id ? onToggleImageZoom : undefined
                      }
                    >
                      <img
                        src={treqSendFileSrc(asset.path)}
                        alt={asset.title}
                        className={cn(
                          "object-contain",
                          asset.id === current?.id && showingImage
                            ? imageZoom === IMAGE_ZOOM_DEFAULT
                              ? "cursor-zoom-in"
                              : "cursor-zoom-out"
                            : undefined,
                        )}
                        style={imageSizeStyle(asset.id)}
                        onLoad={(event) =>
                          rememberFittedBaseSize(asset.id, event.currentTarget)
                        }
                        draggable={false}
                      />
                    </ImageHighlightLayer>
                  </div>
                </div>
              </div>
            ) : (
              <TextAssetLineReview
                content={textByPath[asset.path] ?? "Loading…"}
                comments={asset.id === current?.id ? lineComments : []}
                onCommentsChange={(next) => {
                  if (!current || asset.id !== current.id) return;
                  onLineCommentsChange(next);
                }}
                onUnsavedComposerChange={
                  asset.id === current?.id ? onLineComposerChange : undefined
                }
              />
            )}
          </CarouselItem>
        ))}
      </CarouselContent>
      <CarouselPrevious
        data-lightbox-content
        className="fixed left-3 top-1/2 h-16 w-16 border-white/20 bg-black/40 text-white hover:bg-black/60 hover:text-white [&_svg]:size-8"
      />
      <CarouselNext
        data-lightbox-content
        className="fixed right-3 top-1/2 h-16 w-16 border-white/20 bg-black/40 text-white hover:bg-black/60 hover:text-white [&_svg]:size-8"
      />
    </Carousel>
  );
}
