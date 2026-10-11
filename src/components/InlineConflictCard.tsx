import {
  type Dispatch,
  Fragment,
  type SetStateAction,
  useEffect,
  useRef,
} from "react";
import type {
  ConflictLineRole,
  ConflictRegion,
  ConflictStyle,
} from "../lib/api";
import { getConflictDeletedSides } from "../lib/conflict-deleted-sides";
import type {
  ConflictComment,
  DiffSearchData,
} from "./changes-diff-viewer/types";
import { Button } from "./ui/button";
import { highlightInHtml } from "../lib/text-search";
import { cn } from "../lib/utils";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "./ui/tooltip";
import { Pencil, X } from "lucide-react";
import { ConflictCommentCard } from "./ConflictCommentCard";
import { CommentEditInput } from "./CommentEditInput";

interface InlineConflictCardProps {
  region: ConflictRegion;
  conflictComments: Map<string, ConflictComment>;
  openConflictComments: Set<string>;
  editingConflictCommentId: string | null;
  saveConflictComment: (args: {
    conflictId: string;
    filePath: string;
    conflictNumber: number;
    text: string;
  }) => void;
  clearConflictComment: (conflictId: string) => void;
  toggleConflictComment: (conflictId: string) => void;
  setOpenConflictComments: Dispatch<SetStateAction<Set<string>>>;
  startEditConflictComment: (conflictId: string) => void;
  cancelEditConflictComment: () => void;
  saveEditConflictComment: (conflictId: string, newText: string) => void;
  searchData: DiffSearchData;
  debouncedSearchQuery: string;
  currentMatchIndex: number;
  className?: string;
  registerFileRef?: (el: HTMLDivElement | null) => void;
}

const getConflictLineBackground = (role: ConflictLineRole): string => {
  if (role === "left") return "bg-red-500/20";
  if (role === "right") return "bg-emerald-500/20";
  if (role === "base") return "bg-amber-500/20";
  return "";
};

/**
 * Marker styles whose sections map one-to-one onto side #1 / base / side #2.
 * jj's own `diff` and `snapshot` markers describe themselves in the marker text
 * (and `%%%%%%%` opens a diff, not a side), so they are left unlabelled.
 */
const SIDE_LABELLED_STYLES: ReadonlySet<ConflictStyle> = new Set([
  "git_merge",
  "git_diff3",
  "jj_git_diff3",
]);

const getSectionLabel = (
  line: ConflictRegion["lines"][number],
  markerStyle: ConflictStyle | undefined,
): string | null => {
  // `base` is also the role of the base section's content lines; only the
  // marker that opens a section gets the badge.
  if (line.kind !== "marker") return null;
  if (!markerStyle || !SIDE_LABELLED_STYLES.has(markerStyle)) return null;
  if (line.role === "start") return "Side #1";
  if (line.role === "base") return "Base";
  if (line.role === "separator") return "Side #2";
  return null;
};

const SECTION_LABEL_CLASSES: Record<string, string> = {
  "Side #1": "bg-red-500/20 text-red-700 dark:text-red-300",
  Base: "bg-amber-500/20 text-amber-700 dark:text-amber-300",
  "Side #2": "bg-emerald-500/20 text-emerald-700 dark:text-emerald-300",
};

export const InlineConflictCard = ({
  region,
  conflictComments,
  openConflictComments,
  editingConflictCommentId,
  saveConflictComment,
  clearConflictComment,
  toggleConflictComment,
  setOpenConflictComments,
  startEditConflictComment,
  cancelEditConflictComment,
  saveEditConflictComment,
  searchData,
  debouncedSearchQuery,
  currentMatchIndex,
  className,
  registerFileRef,
}: InlineConflictCardProps) => {
  const cardRef = useRef<HTMLDivElement>(null);
  const lines = region.lines?.length
    ? region.lines
    : region.content.split("\n").map((raw, idx) => ({
        raw,
        kind: "content" as const,
        role: "unknown" as const,
        file_line: region.start_line + idx,
      }));
  const hasComment = conflictComments.has(region.id);
  const deletedSides = getConflictDeletedSides({ ...region, lines });

  useEffect(() => {
    if (!registerFileRef) return;
    const el = cardRef.current;
    if (el) {
      registerFileRef(el);
    }
    return () => {
      registerFileRef(null);
    };
  }, [registerFileRef]);

  const closeConflictInput = () => {
    setOpenConflictComments((prev) => {
      const next = new Set(prev);
      next.delete(region.id);
      return next;
    });
  };

  return (
    <div ref={cardRef} className={className} data-conflict-card={region.id}>
      <div className="flex items-center gap-2 text-xs text-muted-foreground">
        <span className="font-medium text-destructive">
          {region.total_conflicts > 0
            ? `Conflict ${region.conflict_number} of ${region.total_conflicts}`
            : `Conflict ${region.conflict_number}`}
        </span>
        <span>
          lines {region.start_line}–{region.end_line}
        </span>
      </div>
      <div className="p-0">
        <pre className="text-sm font-mono overflow-x-auto bg-muted/30 p-3 rounded whitespace-pre-wrap break-all">
          {lines.map((line, idx) => {
            if (
              line.kind === "content" &&
              ((line.role === "left" && deletedSides.left) ||
                (line.role === "right" && deletedSides.right))
            ) {
              return null;
            }

            const isMarker = line.kind === "marker";
            const bgClass = getConflictLineBackground(line.role);
            const conflictSearchKey = `conflict:${region.id}:${idx}`;
            const conflictLineData =
              searchData.matchesByKey.get(conflictSearchKey);
            let conflictHighlightOffset = -1;
            if (conflictLineData && debouncedSearchQuery) {
              const gf = conflictLineData.firstGlobalIndex;
              if (
                currentMatchIndex >= gf &&
                currentMatchIndex < gf + conflictLineData.count
              ) {
                conflictHighlightOffset = currentMatchIndex - gf;
              }
            }
            const hasSearchHighlight = Boolean(
              debouncedSearchQuery && conflictLineData,
            );
            const lineHtml = hasSearchHighlight
              ? highlightInHtml(
                  line.raw,
                  debouncedSearchQuery,
                  conflictHighlightOffset,
                ).html
              : null;

            const sectionLabel = getSectionLabel(line, region.marker_style);
            const deletedSideLabel =
              sectionLabel === "Side #1" && deletedSides.left
                ? "Side #1"
                : sectionLabel === "Side #2" && deletedSides.right
                  ? "Side #2"
                  : null;

            return (
              <Fragment key={idx}>
                <div
                  data-search-id={conflictSearchKey}
                  data-conflict-line-role={line.role}
                  className={cn(
                    isMarker ? "text-muted-foreground" : "",
                    bgClass,
                  )}
                >
                  {sectionLabel && (
                    <span
                      data-conflict-section-label={sectionLabel}
                      className={cn(
                        "mr-2 rounded px-1.5 py-0.5 text-xs font-sans font-medium",
                        SECTION_LABEL_CLASSES[sectionLabel],
                      )}
                    >
                      {sectionLabel}
                    </span>
                  )}
                  {lineHtml ? (
                    <span dangerouslySetInnerHTML={{ __html: lineHtml }} />
                  ) : (
                    line.raw
                  )}
                </div>
                {deletedSideLabel && (
                  <div
                    data-testid="conflict-deleted-side"
                    data-side={deletedSideLabel}
                    className={cn(
                      "my-1 rounded-md border px-3 py-2 text-sm font-sans",
                      deletedSideLabel === "Side #1"
                        ? "border-red-500/40 bg-red-500/10 text-red-700 dark:text-red-300"
                        : "border-emerald-500/40 bg-emerald-500/10 text-emerald-700 dark:text-emerald-300",
                    )}
                  >
                    {deletedSideLabel} deleted this file
                  </div>
                )}
              </Fragment>
            );
          })}
        </pre>
        {/* In flow, not overlaid: absolute positioning covered the last
				    lines of the conflict. */}
        <div className="mt-2 flex justify-end">
          <Button
            variant="secondary"
            size="sm"
            onClick={() => toggleConflictComment(region.id)}
            aria-label="Add comment"
          >
            Add comment
          </Button>
        </div>
      </div>

      {hasComment && (
        <div className="px-3 py-2 bg-muted/40 border-t border-border">
          <div className="text-muted-foreground mb-1">Resolution note:</div>
          {editingConflictCommentId === region.id ? (
            <div className="bg-background rounded-md p-[12px] border border-border/60">
              <CommentEditInput
                initialText={conflictComments.get(region.id)?.text || ""}
                onSave={(newText) =>
                  saveEditConflictComment(region.id, newText)
                }
                onCancel={cancelEditConflictComment}
                onDiscard={() => {
                  clearConflictComment(region.id);
                  cancelEditConflictComment();
                }}
              />
            </div>
          ) : (
            <TooltipProvider>
              <Tooltip>
                <TooltipTrigger asChild>
                  <div
                    className="group bg-background rounded-md p-[12px] border border-border/60 cursor-pointer hover:shadow-md transition-shadow"
                    onClick={() => startEditConflictComment(region.id)}
                  >
                    <div className="flex items-start gap-2">
                      <Pencil className="w-3 h-3 text-muted-foreground opacity-0 group-hover:opacity-100 transition-opacity flex-shrink-0 mt-0.5" />
                      <p className="text-sm whitespace-pre-wrap flex-1">
                        {conflictComments.get(region.id)?.text}
                      </p>
                      <Tooltip>
                        <TooltipTrigger asChild>
                          <button
                            onClick={(e) => {
                              e.stopPropagation();
                              clearConflictComment(region.id);
                            }}
                            className="p-1 rounded hover:bg-accent text-muted-foreground hover:text-foreground flex-shrink-0"
                          >
                            <X className="w-3 h-3" />
                          </button>
                        </TooltipTrigger>
                        <TooltipContent>Delete comment</TooltipContent>
                      </Tooltip>
                    </div>
                  </div>
                </TooltipTrigger>
                <TooltipContent>Click to edit</TooltipContent>
              </Tooltip>
            </TooltipProvider>
          )}
        </div>
      )}

      {openConflictComments.has(region.id) && (
        <ConflictCommentCard
          conflictId={region.id}
          filePath={region.file_path}
          conflictNumber={region.conflict_number}
          startLine={region.start_line}
          endLine={region.end_line}
          comment={conflictComments.get(region.id)}
          onSave={(text) => {
            saveConflictComment({
              conflictId: region.id,
              filePath: region.file_path,
              conflictNumber: region.conflict_number,
              text,
            });
            closeConflictInput();
          }}
          onClear={() => {
            clearConflictComment(region.id);
            closeConflictInput();
          }}
        />
      )}
    </div>
  );
};
