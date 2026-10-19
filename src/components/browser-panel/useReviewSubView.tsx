import { ChevronDown, FileDiff, Globe } from "lucide-react";
import { useEffect, useState } from "react";
import { usePreviewFeature } from "../../stores/featurePreviewStore";
import type { TreqSendAsset } from "../../lib/treqSend";
import { Button } from "../ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "../ui/dropdown-menu";
import type { BrowserOpenRequest } from "./types";

type ReviewSubView = "diff" | "browser";

/**
 * Browser preview inside the Changes tab: which sub-view is shown, the
 * Diff/Browser switcher, and the tab-row slot the address bar portals into.
 * With the preview off, the sub-view is always "diff" and the switcher is null.
 */
export function useReviewSubView({
  activeTab,
  setActiveTab,
  treqSendAssets,
  dismissTreqSendAsset,
}: {
  activeTab: string;
  setActiveTab: (tab: string) => void;
  /** Pending `treq send` assets, selected from useTreqSendStore by the caller. */
  treqSendAssets: TreqSendAsset[];
  dismissTreqSendAsset: (id: string) => void;
}) {
  const enabled = usePreviewFeature("browser");
  const [subView, setSubView] = useState<ReviewSubView>("diff");
  const [openRequest, setOpenRequest] = useState<BrowserOpenRequest | null>(
    null,
  );
  // Portal target for the browser address bar, rendered in the tab row so
  // it sits alongside the Code/Commits/Changes tabs instead of its own row.
  const [toolbarSlot, setToolbarSlot] = useState<HTMLDivElement | null>(null);

  useEffect(() => {
    if (!enabled && subView === "browser") setSubView("diff");
  }, [enabled, subView]);

  // `treq send --browser <url-or-file>` opens the Browser view directly,
  // instead of showing an attachment preview like image/text sends do.
  useEffect(() => {
    if (!enabled) return;
    const browserAsset = treqSendAssets.find(
      (asset) => asset.mediaType === "browser",
    );
    if (!browserAsset) return;
    setActiveTab("changes");
    setSubView("browser");
    setOpenRequest({ id: browserAsset.id, url: browserAsset.path });
    dismissTreqSendAsset(browserAsset.id);
  }, [enabled, treqSendAssets, dismissTreqSendAsset, setActiveTab]);

  const select = (view: ReviewSubView) => {
    setSubView(view);
    setActiveTab("changes");
  };

  const switcher =
    enabled && activeTab === "changes" ? (
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button
            variant="ghost"
            size="sm"
            className="gap-1.5"
            aria-label="Switch review view"
          >
            {subView === "browser" ? (
              <Globe className="w-4 h-4" />
            ) : (
              <FileDiff className="w-4 h-4" />
            )}
            <span>{subView === "browser" ? "Browser" : "Diff"}</span>
            <ChevronDown className="w-3.5 h-3.5 text-muted-foreground" />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="start" sideOffset={4}>
          <DropdownMenuItem onSelect={() => select("diff")}>
            <FileDiff className="w-4 h-4 mr-2" />
            Diff
          </DropdownMenuItem>
          <DropdownMenuItem onSelect={() => select("browser")}>
            <Globe className="w-4 h-4 mr-2" />
            Browser
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
    ) : null;

  return {
    showBrowser: enabled && subView === "browser",
    /** The switcher is offered, for GPUI to draw while Diff shows. */
    switcherEnabled: enabled && activeTab === "changes",
    select,
    openRequest,
    toolbarSlot,
    setToolbarSlot,
    switcher,
  };
}
