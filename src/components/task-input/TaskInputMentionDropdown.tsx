import { FileText } from "lucide-react";
import { cn } from "../../lib/utils";
import type { FileSearchResult } from "../../lib/api";

interface TaskInputMentionDropdownProps {
  mentionRef: React.RefObject<HTMLDivElement | null>;
  mentionQuery: string | null;
  mentionResults: FileSearchResult[];
  mentionIndex: number;
  onMentionSelect: (file: FileSearchResult) => void;
}

export const TaskInputMentionDropdown: React.FC<
  TaskInputMentionDropdownProps
> = ({
  mentionRef,
  mentionQuery,
  mentionResults,
  mentionIndex,
  onMentionSelect,
}) => (
  <div
    className="grid transition-[grid-template-rows] duration-200 ease-out"
    style={{
      gridTemplateRows:
        mentionQuery !== null && mentionResults.length > 0 ? "1fr" : "0fr",
    }}
  >
    <div className="overflow-hidden">
      <div
        ref={mentionRef}
        className="mx-2 mb-1 rounded-lg border border-border bg-muted/50 overflow-hidden"
      >
        <div className="px-3 py-1 text-xs text-muted-foreground font-medium uppercase tracking-wider border-b border-border">
          Files
        </div>
        {mentionResults.map((file, i) => (
          <button
            key={file.file_path}
            type="button"
            className={cn(
              "w-full px-3 py-1.5 flex items-center gap-2 text-sm text-left transition-colors",
              i === mentionIndex ? "bg-accent/50" : "hover:bg-accent/30",
            )}
            onMouseDown={(e) => {
              e.preventDefault();
              onMentionSelect(file);
            }}
          >
            <FileText className="w-4 h-4 text-muted-foreground flex-shrink-0" />
            <span className="truncate font-mono text-xs">
              {file.relative_path}
            </span>
          </button>
        ))}
      </div>
    </div>
  </div>
);
