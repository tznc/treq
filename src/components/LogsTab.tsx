import { useEffect, useState } from "react";
import useSWR from "swr";
import { Database, Loader2, Table2 } from "lucide-react";
import { Button } from "./ui/button";
import {
  getAgentChat,
  getAppSetupLogs,
  getLogTimeseries,
  getRepoLogs,
  listAgentChats,
} from "../lib/api";
import type { LogRecordView } from "../lib/api-types";
import { LogLevelFilter } from "./LogLevelFilter";
import { LogsSqlExplorer } from "./LogsSqlExplorer";
import { LogFeed } from "./LogFeed";
import { LogsTimeseriesChart } from "./LogsTimeseriesChart";
import { buildLogLinesPrompt } from "../lib/logs-prompt";
import { agentChatToLogRecords } from "../lib/agentChatLogs";

interface Props {
  repoPath: string;
  onSendToAgent?: (prompt: string) => void;
}

type View = "browse" | "explorer";
type SourceGroup = "checks" | "agent-chats" | "app-setup";

/** Repo-level check and agent logs, plus app-level setup script logs. */
export function LogsTab({ repoPath, onSendToAgent }: Props) {
  const [source, setSource] = useState<SourceGroup>("checks");
  const [view, setView] = useState<View>("browse");
  const [levels, setLevels] = useState<string[]>([]);
  const [search, setSearch] = useState("");

  const browsing = source === "checks" && view === "browse";
  const filters = {
    levels: levels.length > 0 ? levels : undefined,
    search: search || undefined,
  };

  const { data: records = [], isLoading } = useSWR(
    browsing ? ["repo-logs", repoPath, levels, search] : null,
    () => getRepoLogs(repoPath, filters),
  );

  const { data: buckets = [] } = useSWR(
    browsing ? ["repo-logs-timeseries", repoPath, levels, search] : null,
    () => getLogTimeseries(repoPath, { ...filters, bucketSeconds: 1 }),
  );

  function handleSendToAgent(chosen: LogRecordView[]) {
    onSendToAgent?.(buildLogLinesPrompt(chosen, "my check logs"));
  }

  const checksSelected = source === "checks";

  return (
    <div data-testid="logs-tab" className="flex flex-col h-full">
      <div className="flex items-center justify-between gap-2 px-4 py-3 border-b">
        <select
          aria-label="Log source"
          className="h-8 rounded-md border bg-background px-2 text-sm"
          value={source}
          onChange={(event) => {
            setSource(event.target.value as SourceGroup);
            setView("browse");
          }}
        >
          <option value="checks">Checks</option>
          <option value="agent-chats">Agent</option>
          <option value="app-setup">App setup</option>
        </select>
        {checksSelected && (
          <div className="flex items-center gap-1">
            <Button
              size="sm"
              variant={browsing ? "secondary" : "ghost"}
              onClick={() => setView("browse")}
            >
              <Table2 className="h-3 w-3 mr-1" />
              Browse
            </Button>
            <Button
              size="sm"
              variant={browsing ? "ghost" : "secondary"}
              onClick={() => setView("explorer")}
            >
              <Database className="h-3 w-3 mr-1" />
              Logs Explorer
            </Button>
          </div>
        )}
      </div>

      {source === "agent-chats" ? (
        <AgentChatsSource repoPath={repoPath} onSendToAgent={onSendToAgent} />
      ) : source === "app-setup" ? (
        <AppSetupSource onSendToAgent={onSendToAgent} />
      ) : browsing ? (
        <>
          <div className="flex flex-wrap items-center gap-2 px-4 py-2 border-b bg-muted/30">
            <LogLevelFilter value={levels} onChange={setLevels} />
            <input
              aria-label="Search logs"
              placeholder="Search all runs…"
              className="ml-auto h-8 w-full max-w-sm min-w-[160px] rounded-md border bg-background px-2 text-sm"
              value={search}
              onChange={(e) => setSearch(e.target.value)}
            />
          </div>

          {buckets.length > 0 && (
            <div className="px-2 pt-2 border-b">
              <LogsTimeseriesChart buckets={buckets} bucketSeconds={1} />
            </div>
          )}

          {isLoading ? (
            <div className="flex items-center gap-2 p-3 text-xs text-muted-foreground">
              <Loader2 className="h-4 w-4 animate-spin" />
              Loading logs…
            </div>
          ) : (
            <LogFeed
              records={records}
              testId="repo-logs-output"
              lineTestId="repo-log-line"
              emptyMessage="No check logs recorded yet. Run a workflow check from the Checks tab of any workspace to populate this table."
              onSendToAgent={handleSendToAgent}
              prefixColumns={[
                {
                  header: "Run",
                  className: "w-[5ch] text-muted-foreground",
                  render: (record) => `#${record.run_id}`,
                },
                {
                  header: "Job",
                  className: "w-[10ch] text-blue-600 dark:text-blue-400",
                  render: (record) => record.job_id,
                },
              ]}
            />
          )}
        </>
      ) : (
        <LogsSqlExplorer repoPath={repoPath} onSendToAgent={onSendToAgent} />
      )}
    </div>
  );
}

function AgentChatsSource({
  repoPath,
  onSendToAgent,
}: {
  repoPath: string;
  onSendToAgent?: (prompt: string) => void;
}) {
  const [selectedId, setSelectedId] = useState<number | null>(null);
  const [search, setSearch] = useState("");

  const { data: chats = [], isLoading: listing } = useSWR(
    ["agent-chats", repoPath],
    () => listAgentChats(repoPath),
  );

  useEffect(() => {
    if (selectedId != null) return;
    if (chats.length > 0) setSelectedId(chats[0].session_id);
  }, [chats, selectedId]);

  const { data: chat, isLoading } = useSWR(
    selectedId != null ? ["agent-chat", repoPath, selectedId] : null,
    () => getAgentChat(repoPath, selectedId as number),
  );

  const records = (chat ? agentChatToLogRecords(chat) : []).filter((record) =>
    search ? record.body.toLowerCase().includes(search.toLowerCase()) : true,
  );

  function handleSendToAgent(chosen: LogRecordView[]) {
    const label = chat ? `agent chat "${chat.name}"` : "an agent chat";
    onSendToAgent?.(buildLogLinesPrompt(chosen, label));
  }

  return (
    <>
      <div className="flex flex-wrap items-center gap-2 px-4 py-2 border-b bg-muted/30">
        <select
          aria-label="Agent terminal"
          className="h-8 rounded-md border bg-background px-2 text-sm min-w-[12rem]"
          value={selectedId ?? ""}
          onChange={(e) =>
            setSelectedId(e.target.value === "" ? null : Number(e.target.value))
          }
          disabled={chats.length === 0}
        >
          {chats.length === 0 ? (
            <option value="">No agent terminals</option>
          ) : (
            chats.map((summary) => (
              <option key={summary.session_id} value={summary.session_id}>
                {summary.name} · {summary.agent}
              </option>
            ))
          )}
        </select>
        <input
          aria-label="Search agent chat"
          placeholder="Search this conversation…"
          className="ml-auto h-8 w-full max-w-sm min-w-[160px] rounded-md border bg-background px-2 text-sm"
          value={search}
          onChange={(e) => setSearch(e.target.value)}
        />
      </div>
      {listing || isLoading ? (
        <div className="flex items-center gap-2 p-3 text-xs text-muted-foreground">
          <Loader2 className="h-4 w-4 animate-spin" />
          Loading agent chats…
        </div>
      ) : (
        <LogFeed
          records={records}
          testId="agent-chat-output"
          lineTestId="agent-chat-line"
          emptyMessage="No agent chat logs yet. Open an agent terminal (not a shell) to start recording the conversation."
          onSendToAgent={handleSendToAgent}
          prefixColumns={[
            {
              header: "Role",
              className: "w-[6ch] text-blue-600 dark:text-blue-400",
              render: (record) => record.job_id,
            },
          ]}
        />
      )}
    </>
  );
}

function AppSetupSource({
  onSendToAgent,
}: {
  onSendToAgent?: (prompt: string) => void;
}) {
  const [levels, setLevels] = useState<string[]>([]);
  const [search, setSearch] = useState("");

  const { data: records = [], isLoading } = useSWR(
    ["app-setup-logs", levels, search],
    () =>
      getAppSetupLogs({
        levels: levels.length > 0 ? levels : undefined,
        search: search || undefined,
      }),
  );

  function handleSendToAgent(chosen: LogRecordView[]) {
    onSendToAgent?.(buildLogLinesPrompt(chosen, "my app setup script logs"));
  }

  return (
    <>
      <div className="flex flex-wrap items-center gap-2 px-4 py-2 border-b bg-muted/30">
        <LogLevelFilter value={levels} onChange={setLevels} />
        <input
          aria-label="Search app setup logs"
          placeholder="Search all runs…"
          className="ml-auto h-8 w-full max-w-sm min-w-[160px] rounded-md border bg-background px-2 text-sm"
          value={search}
          onChange={(e) => setSearch(e.target.value)}
        />
      </div>
      {isLoading ? (
        <div className="flex items-center gap-2 p-3 text-xs text-muted-foreground">
          <Loader2 className="h-4 w-4 animate-spin" />
          Loading logs…
        </div>
      ) : (
        <LogFeed
          records={records}
          testId="app-setup-logs-output"
          lineTestId="app-setup-log-line"
          emptyMessage="No app setup script runs yet. Set a script in Application settings and click Run."
          onSendToAgent={handleSendToAgent}
          prefixColumns={[
            {
              header: "Run",
              className: "w-[5ch] text-muted-foreground",
              render: (record) => `#${record.run_id}`,
            },
          ]}
        />
      )}
    </>
  );
}
