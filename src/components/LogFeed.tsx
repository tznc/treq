import { useEffect, useMemo, useRef, useState } from "react";
import { Bot, CheckSquare, Square } from "lucide-react";
import { Button } from "./ui/button";
import type { LogRecordView } from "../lib/api-types";
import { cn } from "../lib/utils";
import { useLineSelection } from "../hooks/useLineSelection";
import { SearchOverlay } from "./SearchOverlay";

function severityClass(severityText: string): string {
  if (severityText === "ERROR") return "text-red-600 dark:text-red-400";
  if (severityText === "WARN") return "text-amber-600 dark:text-amber-400";
  return "text-foreground";
}
function formatTimestamp(timestamp: string): string {
  const parsed = new Date(timestamp);
  if (Number.isNaN(parsed.getTime())) return timestamp;
  return parsed.toISOString().slice(11, 23);
}
const TIMESTAMP_COL_CLASS = "w-[calc(12ch+1.5rem)]";
const LEVEL_COL_CLASS = "w-[calc(6ch+1.5rem)]";
const CELL_CLASS = "px-3 py-0.5 truncate whitespace-nowrap text-left";
const HEADER_CELL_CLASS = cn(CELL_CLASS, "py-1 font-normal");
export interface PrefixColumn {
  header: string;
  /** Column width class including the cells' 1.5rem padding, e.g. `w-[calc(10ch+1.5rem)]`. */
  width: string;
  /** Extra classes for the column's cells. */
  className?: string;
  render: (record: LogRecordView) => React.ReactNode;
}
interface Props {
  records: LogRecordView[];
  prefixColumns?: PrefixColumn[];
  testId: string;
  lineTestId: string;
  emptyMessage: React.ReactNode;
  onSendToAgent: (records: LogRecordView[]) => void;
}

export function LogFeed({
  records,
  prefixColumns = [],
  testId,
  lineTestId,
  emptyMessage,
  onSendToAgent,
}: Props) {
  const {
    selected,
    multiSelect,
    onLineMouseDown,
    onLineMouseEnter,
    clear,
    toggleMultiSelect,
    selectAll,
  } = useLineSelection(records.length);
  const feedRef = useRef<HTMLDivElement>(null);
  const [findVisible, setFindVisible] = useState(false);
  const [findQuery, setFindQuery] = useState("");
  const [activeMatch, setActiveMatch] = useState(0);
  const [selectionToolbar, setSelectionToolbar] = useState<{
    text: string;
    x: number;
    y: number;
  } | null>(null);
  const matches = useMemo(
    () =>
      findQuery.trim()
        ? records.flatMap((record, index) =>
            record.body.toLowerCase().includes(findQuery.toLowerCase())
              ? [index]
              : [],
          )
        : [],
    [findQuery, records],
  );

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "f") {
        event.preventDefault();
        setFindVisible(true);
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);

  useEffect(() => {
    const onMouseUp = () => {
      const selection = window.getSelection();
      const text = selection?.toString().trim() ?? "";
      if (
        !text ||
        !selection?.rangeCount ||
        !feedRef.current?.contains(selection.anchorNode)
      ) {
        setSelectionToolbar(null);
        return;
      }
      const rect = selection.getRangeAt(0).getBoundingClientRect();
      setSelectionToolbar({ text, x: rect.left + rect.width / 2, y: rect.top });
    };
    document.addEventListener("mouseup", onMouseUp);
    return () => document.removeEventListener("mouseup", onMouseUp);
  }, []);

  function chosenRecords() {
    return Array.from(selected)
      .sort((a, b) => a - b)
      .map((index) => records[index])
      .filter(Boolean);
  }
  function moveMatch(delta: number) {
    if (!matches.length) return;
    const next = (activeMatch + delta + matches.length) % matches.length;
    setActiveMatch(next);
    const matchEls =
      feedRef.current?.querySelectorAll<HTMLElement>("[data-find-match]");
    matchEls?.[next]?.scrollIntoView({ block: "center" });
  }
  function highlightedBody(body: string) {
    if (!findQuery.trim()) return body;
    const escaped = findQuery.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
    return body.split(new RegExp(`(${escaped})`, "gi")).map((part, index) =>
      part.toLowerCase() === findQuery.toLowerCase() ? (
        <mark key={`${part}-${index}`} className="bg-yellow-300 text-black">
          {part}
        </mark>
      ) : (
        part
      ),
    );
  }

  if (!records.length)
    return (
      <div
        data-testid={testId}
        className="flex-1 overflow-auto font-mono text-xs p-3 text-muted-foreground"
      >
        {emptyMessage}
      </div>
    );
  const inspected =
    selected.size === 1 ? records[Array.from(selected)[0]] : null;

  return (
    <div className="flex-1 flex flex-col min-h-0">
      <div className="flex items-center gap-2 px-3 py-1.5 border-b bg-muted/30 text-xs">
        <Button
          size="sm"
          variant={multiSelect ? "secondary" : "ghost"}
          data-testid="multi-select-toggle"
          aria-pressed={multiSelect}
          onClick={toggleMultiSelect}
        >
          {multiSelect ? (
            <CheckSquare className="h-3 w-3 mr-1" />
          ) : (
            <Square className="h-3 w-3 mr-1" />
          )}
          Multi-select
        </Button>
        <Button size="sm" variant="ghost" onClick={selectAll}>
          Select all
        </Button>
        {selected.size > 0 && (
          <>
            <span
              data-testid="selection-count"
              className="text-muted-foreground"
            >
              {selected.size} selected
            </span>
            <Button size="sm" variant="ghost" onClick={clear}>
              Clear
            </Button>
          </>
        )}
      </div>
      <div className="relative flex-1 min-h-0">
        <SearchOverlay
          isVisible={findVisible}
          query={findQuery}
          onQueryChange={(query) => {
            setFindQuery(query);
            setActiveMatch(0);
          }}
          onNext={() => moveMatch(1)}
          onPrevious={() => moveMatch(-1)}
          onClose={() => setFindVisible(false)}
          currentMatch={matches.length ? activeMatch + 1 : 0}
          totalMatches={matches.length}
          className="absolute right-3 top-2 z-20"
        />
        <div
          ref={feedRef}
          data-testid={testId}
          className="h-full overflow-auto font-mono text-xs leading-relaxed select-text"
        >
          <table className="w-full table-fixed border-collapse">
            <colgroup>
              <col className={TIMESTAMP_COL_CLASS} />
              {prefixColumns.map((col) => (
                <col key={col.header} className={col.width} />
              ))}
              <col className={LEVEL_COL_CLASS} />
              <col />
            </colgroup>
            <thead className="sticky top-0 z-10 bg-muted text-muted-foreground uppercase tracking-wide select-none">
              <tr className="border-b">
                <th className={HEADER_CELL_CLASS}>Timestamp</th>
                {prefixColumns.map((col) => (
                  <th key={col.header} className={HEADER_CELL_CLASS}>
                    {col.header}
                  </th>
                ))}
                <th className={HEADER_CELL_CLASS}>Level</th>
                <th className={HEADER_CELL_CLASS}>Message</th>
              </tr>
            </thead>
            <tbody>
              {records.map((record, index) => (
                <tr
                  tabIndex={0}
                  key={`${record.run_id}-${record.job_id}-${record.timestamp}-${index}`}
                  data-testid={lineTestId}
                  data-level={record.severity_text}
                  data-selected={selected.has(index) ? "true" : undefined}
                  aria-selected={selected.has(index)}
                  onMouseDown={() => onLineMouseDown(index)}
                  onMouseEnter={() => onLineMouseEnter(index)}
                  className={cn(
                    "cursor-pointer",
                    selected.has(index) ? "bg-primary/20" : "hover:bg-muted/50",
                  )}
                >
                  <td
                    className={cn(
                      CELL_CLASS,
                      "select-none text-muted-foreground tabular-nums",
                    )}
                  >
                    {formatTimestamp(record.timestamp)}
                  </td>
                  {prefixColumns.map((col) => (
                    <td
                      key={col.header}
                      className={cn(CELL_CLASS, "select-none", col.className)}
                    >
                      {col.render(record)}
                    </td>
                  ))}
                  <td
                    className={cn(
                      CELL_CLASS,
                      "select-none",
                      severityClass(record.severity_text),
                    )}
                  >
                    {record.severity_text}
                  </td>
                  <td
                    data-find-match={
                      matches.includes(index) ? "true" : undefined
                    }
                    className={CELL_CLASS}
                  >
                    {highlightedBody(record.body)}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </div>
      {inspected && (
        <aside
          data-testid="log-inspect-panel"
          className="border-t bg-muted/20 p-3 text-xs"
        >
          <div className="flex items-center justify-between gap-2">
            <strong>Log line details</strong>
            <Button
              size="sm"
              variant="outline"
              data-testid="send-to-agent"
              onClick={() => onSendToAgent(chosenRecords())}
            >
              <Bot className="h-3 w-3 mr-1" />
              Send to agent
            </Button>
          </div>
          <pre className="mt-2 max-h-32 overflow-auto whitespace-pre-wrap select-text">
            {inspected.body}
          </pre>
        </aside>
      )}
      {selectionToolbar && (
        <Button
          size="sm"
          className="fixed z-50 -translate-x-1/2 -translate-y-full shadow-lg"
          style={{ left: selectionToolbar.x, top: selectionToolbar.y - 6 }}
          onMouseDown={(event) => event.preventDefault()}
          onClick={() => {
            onSendToAgent([{ ...records[0], body: selectionToolbar.text }]);
            setSelectionToolbar(null);
          }}
        >
          <Bot className="h-3 w-3 mr-1" />
          Send selection to agent
        </Button>
      )}
    </div>
  );
}
