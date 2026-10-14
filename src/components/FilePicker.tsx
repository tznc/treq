import { useState } from "react";
import useSWR from "swr";
import { Command } from "cmdk";
import { FileText } from "lucide-react";
import { ensureWorkspaceIndexed, searchWorkspaceFiles } from "../lib/api";
import { useDebounce } from "../hooks/useDebounce";

interface FilePickerProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  repoPath: string;
  workspaceId: number | null;
  /** Absolute path to index and search (workspace checkout or repo root). */
  workspacePath: string;
  onFileSelect: (relativePath: string) => void;
}

export const FilePicker: React.FC<FilePickerProps> = ({
  open,
  onOpenChange,
  repoPath,
  workspaceId,
  workspacePath,
  onFileSelect,
}) => {
  const [query, setQuery] = useState("");
  const debouncedQuery = useDebounce(query, 150);

  const { data: indexed } = useSWR(
    open
      ? [
          "ensure-workspace-indexed",
          repoPath,
          workspaceId,
          workspacePath || repoPath,
        ]
      : null,
    () =>
      ensureWorkspaceIndexed(
        repoPath,
        workspaceId,
        workspacePath || repoPath,
      ).then(() => true),
  );

  const { data: results = [] } = useSWR(
    open && indexed
      ? ["search-workspace-files", repoPath, workspaceId, debouncedQuery]
      : null,
    () => searchWorkspaceFiles(repoPath, workspaceId, debouncedQuery, 50),
  );

  const handleSelect = (filePath: string) => {
    onFileSelect(filePath);
    onOpenChange(false);
  };

  const getFileIcon = () => (
    <FileText className="w-4 h-4 text-muted-foreground" />
  );

  if (!open) {
    return null;
  }

  return (
    <Command.Dialog
      open={open}
      onOpenChange={onOpenChange}
      shouldFilter={false}
      label="Jump to File"
      className="[&_[cmdk-root]]:bg-background [&_[cmdk-root]]:text-foreground"
    >
      <span className="sr-only">
        <h2>Jump to File</h2>
        <p>Jump to file</p>
      </span>
      <div
        data-testid="modal"
        className="bg-background text-foreground rounded-xl border border-border shadow-2xl w-[40vw] max-w-none overflow-hidden"
      >
        <div className="flex items-center border-b border-border px-3 bg-background">
          <Command.Input
            placeholder="Search files..."
            value={query}
            onValueChange={setQuery}
            className="border-0 focus-visible:ring-0 focus-visible:ring-offset-0 h-12 flex-1 bg-transparent outline-none text-sm placeholder:text-muted-foreground text-foreground"
          />
        </div>

        <Command.List className="max-h-[300px] overflow-y-auto py-2">
          {results.length === 0 && (
            <Command.Empty>
              <div className="px-4 py-8 text-center text-muted-foreground text-sm">
                {query.trim() ? "No files found" : "Type to search files..."}
              </div>
            </Command.Empty>
          )}

          {results.map((file) => (
            <Command.Item
              key={file.file_path}
              value={file.relative_path}
              onSelect={() => handleSelect(file.relative_path)}
              className="px-3 py-1.5 mx-2 rounded-md flex items-center gap-3 cursor-pointer text-foreground aria-selected:bg-accent/50 aria-selected:text-foreground hover:bg-accent/30 transition-colors"
            >
              {getFileIcon()}
              <span className="truncate font-mono text-sm">
                {file.relative_path}
              </span>
            </Command.Item>
          ))}
        </Command.List>

        <div className="border-t border-border px-3 py-2 flex items-center justify-between text-sm text-muted-foreground">
          <div className="flex items-center gap-3">
            <span>
              <kbd className="px-1.5 py-0.5 bg-muted rounded text-xs">↑↓</kbd>{" "}
              Navigate
            </span>
            <span>
              <kbd className="px-1.5 py-0.5 bg-muted rounded text-xs">↵</kbd>{" "}
              Open
            </span>
            <span>
              <kbd className="px-1.5 py-0.5 bg-muted rounded text-xs">Esc</kbd>{" "}
              Close
            </span>
          </div>
        </div>
      </div>
    </Command.Dialog>
  );
};
