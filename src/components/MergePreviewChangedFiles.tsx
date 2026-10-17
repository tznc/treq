import { useState } from "react";
import { ChevronDown, ChevronRight, FileText } from "lucide-react";
import type { getWorkspaceDiff } from "../lib/api";
import { cn } from "../lib/utils";

type WorkspaceDiff = Awaited<ReturnType<typeof getWorkspaceDiff>>;

/** Combined diff of the merge, every file expanded until collapsed. */
export const MergePreviewChangedFiles = ({
  diff,
}: {
  diff: WorkspaceDiff | null;
}) => {
  const [collapsed, setCollapsed] = useState<Set<string>>(() => new Set());
  const toggleFile = (path: string) => {
    setCollapsed((prev) => {
      const next = new Set(prev);
      if (next.has(path)) {
        next.delete(path);
      } else {
        next.add(path);
      }
      return next;
    });
  };

  return (
    <section>
      <h2 className="text-sm font-semibold mb-3">
        Changed Files ({diff?.committed_files.length || 0})
      </h2>
      {diff && diff.committed_files.length > 0 ? (
        <div className="border rounded-lg divide-y">
          {diff.hunks_by_file.map((fileDiff) => {
            const isExpanded = !collapsed.has(fileDiff.path);
            return (
              <div key={fileDiff.path}>
                <button
                  className="w-full p-3 flex items-center gap-2 text-left hover:bg-muted/50 transition-colors"
                  onClick={() => toggleFile(fileDiff.path)}
                  type="button"
                >
                  {isExpanded ? (
                    <ChevronDown className="w-4 h-4 text-muted-foreground flex-shrink-0" />
                  ) : (
                    <ChevronRight className="w-4 h-4 text-muted-foreground flex-shrink-0" />
                  )}
                  <FileText className="w-4 h-4 text-muted-foreground flex-shrink-0" />
                  <span className="font-mono text-sm flex-1 truncate">
                    {fileDiff.path}
                  </span>
                </button>
                {isExpanded && fileDiff.hunks.length > 0 && (
                  <div className="border-t bg-muted/30">
                    {fileDiff.hunks.map((hunk, hunkIndex) => (
                      <pre
                        key={hunk.id || hunkIndex}
                        className="p-2 text-xs font-mono overflow-x-auto"
                      >
                        <div className="text-muted-foreground mb-1">
                          {hunk.header}
                        </div>
                        {hunk.lines.map((line, lineIndex) => (
                          <div
                            key={lineIndex}
                            className={cn(
                              line.startsWith("+") &&
                                !line.startsWith("+++") &&
                                "bg-emerald-500/20",
                              line.startsWith("-") &&
                                !line.startsWith("---") &&
                                "bg-red-500/20",
                            )}
                          >
                            {line}
                          </div>
                        ))}
                      </pre>
                    ))}
                  </div>
                )}
              </div>
            );
          })}
        </div>
      ) : (
        <div className="border rounded-lg p-6 text-center text-muted-foreground">
          No file changes
        </div>
      )}
    </section>
  );
};
