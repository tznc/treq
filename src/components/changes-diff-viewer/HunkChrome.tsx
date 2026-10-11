import { ChevronDown, ChevronUp } from "lucide-react";
import type { JjDiffHunk } from "../../lib/api";
import { HighlightedLine } from "../diff/HighlightedLine";
import { parseHunkHeader } from "../diff/hunk-lines";
import { cn } from "../../lib/utils";
import { getLanguageFromPath } from "../../lib/syntax-highlight";

export function HunkHeaderRow({
  hunk,
  onExpandBefore,
}: {
  hunk: JjDiffHunk;
  onExpandBefore?: () => void;
}) {
  return (
    <div
      className={cn(
        "flex items-stretch font-mono text-sm",
        "bg-muted/60",
        onExpandBefore && "cursor-pointer hover:bg-muted/80",
      )}
      onClick={onExpandBefore}
    >
      <div className="w-24 flex-shrink-0 border-r border-border/40" />
      <div className="w-6 flex-shrink-0" />
      <div className="w-5 flex-shrink-0" />
      <div className="flex-1 flex items-center px-[8px] py-[2px]">
        {onExpandBefore && <ChevronUp className="w-3 h-3 mr-1" />}
        <span className="text-muted-foreground truncate">{hunk.header}</span>
        {onExpandBefore && (
          <span className="text-muted-foreground text-xs ml-2">
            Show more lines above
          </span>
        )}
      </div>
    </div>
  );
}

export function HunkExpandControl({
  direction,
  onExpand,
}: {
  direction: "before" | "after";
  onExpand: () => void;
}) {
  return (
    <div
      className="flex items-stretch font-mono text-sm bg-muted/30 hover:bg-muted/50 cursor-pointer transition-colors"
      onClick={onExpand}
    >
      <div className="w-24 flex-shrink-0 border-r border-border/40" />
      <div className="w-6 flex-shrink-0" />
      <div className="w-5 flex-shrink-0" />
      <div className="flex-1 flex items-center justify-center px-[8px] py-[2px] gap-1 text-muted-foreground text-xs">
        {direction === "before" ? (
          <ChevronUp className="w-3 h-3" />
        ) : (
          <ChevronDown className="w-3 h-3" />
        )}
        <span>
          {direction === "before"
            ? "Show more lines above"
            : "Show more lines below"}
        </span>
      </div>
    </div>
  );
}

export function HunkContextLineRow({
  filePath,
  line,
  lineNum,
}: {
  filePath: string;
  line: string;
  lineNum: number;
}) {
  const language = getLanguageFromPath(filePath);
  return (
    <div className="flex items-stretch font-mono text-sm">
      <div className="w-24 flex-shrink-0 text-muted-foreground select-none border-r border-border/40 flex items-center">
        <span className="w-10 text-right text-sm mr-1">
          {lineNum > 0 ? lineNum : ""}
        </span>
        <span className="w-10 text-right text-sm">
          {lineNum > 0 ? lineNum : ""}
        </span>
      </div>
      <div className="w-6 flex-shrink-0" />
      <div className="w-5 flex-shrink-0 text-center select-none"> </div>
      <div className="flex-1 px-[8px] py-[2px] whitespace-pre-wrap break-all text-muted-foreground">
        <HighlightedLine content={line || " "} language={language} />
      </div>
    </div>
  );
}

export function contextLineNumber(args: {
  hunk: JjDiffHunk;
  direction: "before" | "after";
  ctxIdx: number;
  beforeLength: number;
}): number {
  const { newStart, newCount } = parseHunkHeader(args.hunk.header);
  if (args.direction === "before") {
    return newStart - args.beforeLength + args.ctxIdx;
  }
  return newStart + newCount + args.ctxIdx;
}
