import { Minus, Plus, Undo2 } from "lucide-react";
import { cn } from "../lib/utils";
import { FileContextMenu } from "./FileContextMenu";

interface JjFileChange {
  path: string;
  status: string;
  previous_path?: string | null;
}

export interface GitFileRowProps {
  file: JjFileChange;
  isSelected?: boolean;
  isActive?: boolean;
  isLastSelected?: boolean;
  readOnly?: boolean;
  onFileClick?: (path: string, event: React.MouseEvent) => void;
  onDiscard?: (path: string) => void;
  onStage?: (path: string) => void;
  onUnstage?: (path: string) => void;
  isStaged?: boolean;
  workspacePath?: string;
  /** Enables HTML5 drag of this file (or the current selection) onto a workspace. */
  onFileDragStart?: (path: string, event: React.DragEvent) => void;
}

function formatFileLabel(filePath: string) {
  const parts = filePath.split("/");
  const name = parts.pop() || filePath;
  const directory = parts.length > 0 ? parts.join("/") : null;
  return { name, directory };
}

export const GitFileRow = ({
  file,
  isSelected = false,
  isActive = false,
  isLastSelected = false,
  readOnly = false,
  onFileClick,
  onDiscard,
  onStage,
  onUnstage,
  isStaged = false,
  workspacePath,
  onFileDragStart,
}: GitFileRowProps) => {
  const label = formatFileLabel(file.path);
  const { status } = file;
  const canDrag = !readOnly && !!onFileDragStart;

  return (
    <FileContextMenu filePath={file.path} workspacePath={workspacePath || ""}>
      <div
        className={cn(
          "group/row relative min-w-0 py-1 text-sm flex items-center gap-1 cursor-pointer border-l-2",
          isSelected
            ? "bg-blue-500/60 border-blue-600 font-semibold text-white"
            : "border-transparent hover:bg-accent/20",
        )}
        draggable={canDrag}
        onDragStart={
          canDrag
            ? (e) => {
                onFileDragStart(file.path, e);
              }
            : undefined
        }
        onClick={(e) => {
          onFileClick?.(file.path, e);
        }}
        title={file.path}
      >
        <div className="ml-1 flex-1 flex items-center gap-2 min-w-0 font-sans">
          <span
            className={cn(
              "font-medium min-w-0 truncate",
              isActive && "text-blue-500",
            )}
          >
            {label.name}
          </span>
          {label.directory && (
            <span
              className={cn(
                "min-w-0 flex-1 text-muted-foreground/60 truncate text-xs",
                isActive && "text-blue-400",
              )}
            >
              {label.directory}
            </span>
          )}
        </div>
        <div className="flex items-center gap-1 flex-shrink-0">
          <span className="text-sm font-sans min-w-[1ch] text-muted-foreground">
            {status ?? ""}
          </span>
          {/* Discard button - moved before stage/unstage to avoid misclicks */}
          {isLastSelected && isSelected && !readOnly && onDiscard && (
            <button
              type="button"
              className="p-0.5 hover:text-foreground hover:bg-red-500/20 text-red-600 dark:text-red-400 rounded transition-colors"
              onClick={(e) => {
                e.stopPropagation();
                onDiscard(file.path);
              }}
              title="Discard selected files"
            >
              <Undo2 className="w-3 h-3" />
            </button>
          )}
          {/* Stage button - show only on last selected file */}
          {isLastSelected &&
            isSelected &&
            !readOnly &&
            onStage &&
            !isStaged && (
              <button
                type="button"
                className="p-0.5 opacity-0 group-hover/row:opacity-100 hover:text-foreground hover:bg-accent rounded transition-opacity transition-colors"
                onClick={(e) => {
                  e.stopPropagation();
                  onStage(file.path);
                }}
                title="Stage file(s) for commit"
              >
                <Plus className="w-3 h-3" />
              </button>
            )}
          {/* Unstage button - show on hover */}
          {!readOnly && onUnstage && isStaged && (
            <button
              type="button"
              className="p-0.5 opacity-0 group-hover/row:opacity-100 hover:text-foreground hover:bg-accent rounded transition-opacity transition-colors"
              onClick={(e) => {
                e.stopPropagation();
                onUnstage(file.path);
              }}
              title="Unstage file"
            >
              <Minus className="w-3 h-3" />
            </button>
          )}
        </div>
      </div>
    </FileContextMenu>
  );
};

GitFileRow.displayName = "GitFileRow";
