import { cn } from "../../lib/utils";
import type { JjDiffHunk } from "../../lib/api";
import { AgentReviewInlineList } from "../changes-diff-viewer/AgentReviewContext";
import { computeHunkLineNumbers } from "../diff/hunk-lines";

interface MobileHunkViewProps {
  hunk: JjDiffHunk;
  /** Needed to look up agent review comments anchored to this file's lines. */
  filePath: string;
}

function lineClass(line: string): string {
  if (line.startsWith("+") && !line.startsWith("+++")) {
    return "bg-emerald-500/10 text-emerald-700 dark:text-emerald-400";
  }
  if (line.startsWith("-") && !line.startsWith("---")) {
    return "bg-red-500/10 text-red-700 dark:text-red-400";
  }
  return "text-foreground";
}

/** Read-only, touch-first rendering of a single diff hunk. */
export function MobileHunkView({ hunk, filePath }: MobileHunkViewProps) {
  const lineNumbers = computeHunkLineNumbers(hunk);

  return (
    <div className="rounded-md border overflow-x-auto">
      <div className="border-b bg-muted px-2 py-1 font-mono text-xs text-muted-foreground">
        {hunk.header}
      </div>
      <pre className="text-xs leading-5">
        {hunk.lines.map((line, index) => {
          const numbers = lineNumbers[index];
          const lineSide: "old" | "new" =
            numbers?.new !== undefined ? "new" : "old";
          const lineNumber = numbers?.new ?? numbers?.old;
          return (
            <div key={index}>
              <div className={cn("whitespace-pre px-2", lineClass(line))}>
                {line}
              </div>
              {lineNumber !== undefined && (
                <AgentReviewInlineList
                  filePath={filePath}
                  lineNumber={lineNumber}
                  lineSide={lineSide}
                  hunk={hunk}
                />
              )}
            </div>
          );
        })}
      </pre>
    </div>
  );
}
