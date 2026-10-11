import {
  startTransition,
  useActionState,
  useEffect,
  useRef,
  useState,
} from "react";
import useSWR from "swr";
import { open } from "@tauri-apps/plugin-dialog";
import { FilePicker } from "./FilePicker";
import { TaskInputMentionDropdown } from "./task-input/TaskInputMentionDropdown";
import { TaskInputToolbar } from "./task-input/TaskInputToolbar";
import { IssueChip } from "./task-input/IssueChip";
import {
  createSession,
  ensureWorkspaceIndexed,
  getSetting,
  getRepoSetting,
  searchWorkspaceFiles,
  setRepoSetting,
  type FileSearchResult,
} from "../lib/api";
import {
  formatPromptWithIssue,
  ISSUE_SOURCES,
  type IssueAttachment,
  type IssueSource,
} from "../lib/promptAttachments";
import {
  type IssueWorkspace,
  openOrCreateIssueWorkspace,
} from "../lib/issueWorkspace";
import {
  createTaskWorkspace,
  isNewWorkspaceForTasksOn,
  NEW_WORKSPACE_FOR_TASKS_SETTING,
} from "../lib/taskWorkspace";
import { useToast } from "./ui/toast";
import { useDebounce } from "../hooks/useDebounce";
import { useRepoEnvironment } from "../hooks/useRepoEnvironment";
import { cn } from "../lib/utils";
import { DEFAULT_AGENT, toAgentKind, type AgentKind } from "../lib/agents";
import type { SessionCreationInfo } from "../types/sessions";

interface TaskInputProps {
  repoPath: string;
  workspaceId: number | null;
  workingDirectory: string;
  onSessionCreated?: (session: SessionCreationInfo) => void;
  /** Pre-fill the textarea with this text on mount (e.g. re-running a past prompt). */
  initialText?: string;
  /**
   * Pre-attach an issue from any tracker. Submitting opens (or creates) the
   * issue's own workspace and starts the session there.
   */
  initialIssue?: IssueAttachment | null;
  onIssueChange?: (issue: IssueAttachment | null) => void;
  /**
   * Home view only: show the "Run in a new workspace" toggle (on unless the
   * repo turned it off), which starts each task in a workspace of its own.
   */
  offerNewWorkspace?: boolean;
}

export const TaskInput: React.FC<TaskInputProps> = ({
  repoPath,
  workspaceId,
  workingDirectory,
  onSessionCreated,
  initialText,
  initialIssue = null,
  onIssueChange,
  offerNewWorkspace = false,
}) => {
  const [taskText, setTaskText] = useState(initialText ?? "");
  const [issue, setIssue] = useState<IssueAttachment | null>(initialIssue);
  const [filePickerOpen, setFilePickerOpen] = useState(false);
  const [focused, setFocused] = useState(false);
  const [saveAsRepoDefault, setSaveAsRepoDefault] = useState(false);
  const [showSaveAsRepoDefault, setShowSaveAsRepoDefault] = useState(false);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const mentionRef = useRef<HTMLDivElement>(null);
  const { addToast } = useToast();
  const environment = useRepoEnvironment(repoPath, (message) =>
    addToast({
      title: "Devcontainer did not start",
      description: message,
      type: "error",
    }),
  );

  // @ mention state
  const [mentionQuery, setMentionQuery] = useState<string | null>(null);
  const [mentionIndex, setMentionIndex] = useState(0);
  const [mentionAnchor, setMentionAnchor] = useState<number | null>(null);
  const debouncedMentionQuery = useDebounce(mentionQuery, 150);

  // Autofocus when workspace changes
  useEffect(() => {
    textareaRef.current?.focus();
  }, [workspaceId]);

  const { data: agentSettings } = useSWR(
    ["task-default-agent", repoPath, workspaceId],
    async (): Promise<{ agent: AgentKind; fromRepo: boolean }> => {
      const repoAgent = toAgentKind(
        await getRepoSetting(repoPath, "default_agent"),
      );
      if (repoAgent) return { agent: repoAgent, fromRepo: true as const };
      const agent =
        toAgentKind(await getSetting("default_agent")) ?? DEFAULT_AGENT;
      return { agent, fromRepo: false as const };
    },
  );
  const [agentOverride, setAgentOverride] = useState<AgentKind | null>(null);
  const selectedAgent: AgentKind =
    agentOverride ?? agentSettings?.agent ?? DEFAULT_AGENT;
  const configuredDefaultAgent: AgentKind = agentSettings?.agent ?? "claude";
  const setSelectedAgent = setAgentOverride;

  const { data: newWorkspaceSetting, mutate: mutateNewWorkspaceSetting } =
    useSWR(
      offerNewWorkspace
        ? ["repo-setting", repoPath, NEW_WORKSPACE_FOR_TASKS_SETTING]
        : null,
      () => getRepoSetting(repoPath, NEW_WORKSPACE_FOR_TASKS_SETTING),
    );
  const runInNewWorkspace =
    offerNewWorkspace && isNewWorkspaceForTasksOn(newWorkspaceSetting);
  const setRunInNewWorkspace = (on: boolean) =>
    mutateNewWorkspaceSetting(
      async () => {
        await setRepoSetting(
          repoPath,
          NEW_WORKSPACE_FOR_TASKS_SETTING,
          String(on),
        );
        return String(on);
      },
      { optimisticData: String(on), rollbackOnError: true },
    ).catch((error: unknown) =>
      addToast({
        title: "Failed to save setting",
        description: error instanceof Error ? error.message : String(error),
        type: "error",
      }),
    );

  useEffect(() => {
    setAgentOverride(null);
    setSaveAsRepoDefault(false);
    setShowSaveAsRepoDefault(false);
  }, [repoPath, workspaceId]);

  // Auto-resize textarea
  useEffect(() => {
    const textarea = textareaRef.current;
    if (textarea) {
      textarea.style.height = "auto";
      textarea.style.height = `${Math.min(textarea.scrollHeight, 200)}px`;
    }
  }, [taskText]);

  // The mention search reads the file index, so build it the first time the
  // user types an @ (same key as FilePicker, so the two share one run).
  const { data: mentionIndexReady = false } = useSWR(
    mentionQuery !== null
      ? ["ensure-workspace-indexed", repoPath, workspaceId, workingDirectory]
      : null,
    () =>
      ensureWorkspaceIndexed(repoPath, workspaceId, workingDirectory).then(
        () => true,
      ),
  );

  const { data: mentionResults = [] } = useSWR(
    debouncedMentionQuery !== null && mentionIndexReady
      ? ["task-mentions", repoPath, workspaceId, debouncedMentionQuery]
      : null,
    () =>
      searchWorkspaceFiles(repoPath, workspaceId, debouncedMentionQuery!, 4),
  );

  useEffect(() => {
    setMentionIndex(0);
  }, [mentionResults]);

  // Close mention dropdown on outside click
  useEffect(() => {
    if (mentionQuery === null) return;
    const handler = (e: MouseEvent) => {
      const target = e.target as Node;
      if (
        mentionRef.current &&
        !mentionRef.current.contains(target) &&
        textareaRef.current &&
        !textareaRef.current.contains(target)
      ) {
        setMentionQuery(null);
      }
    };
    document.addEventListener("mousedown", handler);
    return () => document.removeEventListener("mousedown", handler);
  }, [mentionQuery]);

  // Helper: insert text at the current cursor position (or end) and update taskText
  const insertAtCursor = (insertion: string) => {
    const textarea = textareaRef.current;
    const cursorPos = textarea?.selectionStart ?? taskText.length;
    const before = taskText.slice(0, cursorPos);
    const after = taskText.slice(cursorPos);
    // Add a leading space if cursor is not at start and previous char isn't whitespace
    const needsSpace = before.length > 0 && !/\s$/.test(before);
    const text = needsSpace ? ` ${insertion}` : insertion;
    const newText = before + text + after;
    setTaskText(newText);

    const newCursorPos = before.length + text.length;
    requestAnimationFrame(() => {
      if (textarea) {
        textarea.focus();
        textarea.selectionStart = newCursorPos;
        textarea.selectionEnd = newCursorPos;
      }
    });
  };

  // FilePicker: insert @relative_path at cursor
  const handleFileSelect = (relativePath: string) => {
    insertAtCursor(`@${relativePath} `);
  };

  // Finder: insert 'absolute_path' at cursor
  const handleAttachFromFinder = async () => {
    const selected = await open({
      multiple: true,
      title: "Attach Files",
    });
    if (!selected) return;
    const paths = Array.isArray(selected) ? selected : [selected];
    const textarea = textareaRef.current;
    const cursorPos = textarea?.selectionStart ?? taskText.length;
    const before = taskText.slice(0, cursorPos);
    const after = taskText.slice(cursorPos);
    const needsSpace = before.length > 0 && !/\s$/.test(before);
    const insertion = paths.map((p) => `'${p}'`).join(" ");
    const text = `${(needsSpace ? " " : "") + insertion} `;
    const newText = before + text + after;
    setTaskText(newText);

    const newCursorPos = before.length + text.length;
    requestAnimationFrame(() => {
      if (textarea) {
        textarea.focus();
        textarea.selectionStart = newCursorPos;
        textarea.selectionEnd = newCursorPos;
      }
    });
  };

  // @ mention dropdown: insert @relative_path at cursor (preserving the @)
  const handleMentionSelect = (file: FileSearchResult) => {
    if (mentionAnchor === null) return;
    const textarea = textareaRef.current;
    const cursorPos = textarea?.selectionStart ?? taskText.length;
    // Replace from the @ anchor to cursor with @relative_path
    const before = taskText.slice(0, mentionAnchor);
    const after = taskText.slice(cursorPos);
    const inserted = `@${file.relative_path} `;
    const newText = before + inserted + after;
    setTaskText(newText);

    // Close dropdown
    setMentionQuery(null);
    setMentionAnchor(null);

    // Restore focus and cursor position after the inserted path
    requestAnimationFrame(() => {
      if (textarea) {
        textarea.focus();
        const newCursorPos = before.length + inserted.length;
        textarea.selectionStart = newCursorPos;
        textarea.selectionEnd = newCursorPos;
      }
    });
  };

  const handleTextChange = (e: React.ChangeEvent<HTMLTextAreaElement>) => {
    const { value } = e.target;
    setTaskText(value);

    const cursorPos = e.target.selectionStart;
    // Look backwards from cursor to find @ preceded by whitespace or at start
    let foundAnchor: number | null = null;
    for (let i = cursorPos - 1; i >= 0; i--) {
      const ch = value[i];
      if (ch === "@") {
        // Valid if at position 0 or preceded by whitespace
        if (i === 0 || /\s/.test(value[i - 1])) {
          foundAnchor = i;
        }
        break;
      }
      // Stop searching if we hit whitespace (no @ in this "word")
      if (/\s/.test(ch)) break;
    }

    if (foundAnchor !== null) {
      setMentionAnchor(foundAnchor);
      setMentionQuery(value.slice(foundAnchor + 1, cursorPos));
    } else {
      setMentionQuery(null);
      setMentionAnchor(null);
    }
  };

  // Sub-items open one workspace at a time after the parent, but only the
  // parent's workspace opens here, so tell the user what happened to the rest.
  const reportSubItemWorkspaces = (
    source: IssueSource,
    { subItemResults, subItemFailures }: IssueWorkspace,
  ) => {
    const noun = ISSUE_SOURCES[source].subItemNoun.replace(/s$/, "");
    const label = noun.charAt(0).toUpperCase() + noun.slice(1);
    if (subItemResults.length > 0) {
      const created = subItemResults.filter((r) => r.created).length;
      const opened = subItemResults.length - created;
      addToast({
        title: `${label} workspaces ready`,
        description:
          opened > 0
            ? `Created ${created} and opened ${opened} existing.`
            : `Created ${created}.`,
        type: "success",
      });
    }
    if (subItemFailures.length > 0) {
      const count = subItemFailures.length;
      addToast({
        title: `${count} ${noun} workspace${count === 1 ? "" : "s"} failed`,
        description: subItemFailures.map((f) => f.error).join("; "),
        type: "warning",
      });
    }
  };

  const [, submitTask, submitting] = useActionState(
    async (_prev: null, mode: "plan" | "acceptEdits") => {
      const trimmed = taskText.trim();
      if (!trimmed && !issue) return null;

      const pendingPrompt = issue
        ? formatPromptWithIssue(trimmed, issue)
        : trimmed;

      try {
        if (saveAsRepoDefault && selectedAgent !== configuredDefaultAgent) {
          await setRepoSetting(repoPath, "default_agent", selectedAgent);
          setSaveAsRepoDefault(false);
        }

        const sessionName =
          pendingPrompt.length > 50
            ? `${pendingPrompt.slice(0, 47)}...`
            : pendingPrompt;

        const issueWorkspace = issue
          ? await openOrCreateIssueWorkspace(repoPath, issue)
          : null;
        let targetWorkspaceId = workspaceId;
        if (issueWorkspace) {
          targetWorkspaceId = issueWorkspace.workspaceId;
        } else if (runInNewWorkspace) {
          targetWorkspaceId = await createTaskWorkspace(
            repoPath,
            pendingPrompt,
            sessionName,
          );
        }
        if (issue && issueWorkspace)
          reportSubItemWorkspaces(issue.source, issueWorkspace);

        const dbSessionId = await createSession(
          repoPath,
          targetWorkspaceId,
          sessionName,
        );

        const sessionRepoPath = repoPath || workingDirectory;

        onSessionCreated?.({
          sessionId: dbSessionId,
          sessionName,
          workspaceId: targetWorkspaceId,
          workspacePath: null,
          repoPath: sessionRepoPath,
          pendingPrompt,
          permissionMode: mode,
          agent: selectedAgent,
        });

        setTaskText("");
        setIssue(null);
        setShowSaveAsRepoDefault(false);
      } catch (error) {
        addToast({
          title: "Failed to create task",
          description: error instanceof Error ? error.message : String(error),
          type: "error",
        });
      }
      return null;
    },
    null,
  );

  const handleSubmit = (mode: "plan" | "acceptEdits") => {
    if (submitting) return;
    startTransition(() => {
      void submitTask(mode);
    });
  };

  const handleKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    // When mention dropdown is open, handle navigation keys
    if (mentionQuery !== null && mentionResults.length > 0) {
      if (e.key === "ArrowDown") {
        e.preventDefault();
        setMentionIndex((prev) => (prev + 1) % mentionResults.length);
        return;
      }
      if (e.key === "ArrowUp") {
        e.preventDefault();
        setMentionIndex(
          (prev) => (prev - 1 + mentionResults.length) % mentionResults.length,
        );
        return;
      }
      if (e.key === "Enter") {
        e.preventDefault();
        handleMentionSelect(mentionResults[mentionIndex]);
        return;
      }
      if (e.key === "Escape" || e.key === "Tab") {
        setMentionQuery(null);
        setMentionAnchor(null);
        return;
      }
    }

    if (e.key === "Enter" && (e.metaKey || e.ctrlKey) && !e.shiftKey) {
      e.preventDefault();
      handleSubmit("acceptEdits");
    }
  };

  const isEmpty = taskText.trim().length === 0 && !issue;

  return (
    <>
      <div
        className="max-w-2xl min-w-0 mx-auto w-full"
        data-testid="agent-task-input"
      >
        <div
          className={cn(
            "min-h-28 rounded-xl border bg-background relative transition-colors flex flex-col",
            focused ? "border-blue-400" : "border-border",
          )}
        >
          {issue && (
            <IssueChip
              issue={issue}
              onRemove={() => {
                setIssue(null);
                onIssueChange?.(null);
              }}
            />
          )}
          <textarea
            ref={textareaRef}
            value={taskText}
            onChange={handleTextChange}
            onKeyDown={handleKeyDown}
            onFocus={() => setFocused(true)}
            onBlur={() => setFocused(false)}
            placeholder="Describe a task..."
            rows={1}
            className="block w-full min-w-0 resize-none overflow-x-hidden border-0 bg-transparent px-4 pt-3 pb-1 text-sm outline-none placeholder:text-muted-foreground focus:ring-0 caret-blue-400"
            style={{ minHeight: "44px", maxHeight: "200px" }}
          />

          <TaskInputMentionDropdown
            mentionRef={mentionRef}
            mentionQuery={mentionQuery}
            mentionResults={mentionResults}
            mentionIndex={mentionIndex}
            onMentionSelect={handleMentionSelect}
          />

          <div className="mt-auto">
            <TaskInputToolbar
              isEmpty={isEmpty}
              submitting={submitting}
              selectedAgent={selectedAgent}
              configuredDefaultAgent={configuredDefaultAgent}
              saveAsRepoDefault={saveAsRepoDefault}
              showSaveAsRepoDefault={showSaveAsRepoDefault}
              onOpenFilePicker={() => setFilePickerOpen(true)}
              onAttachFromFinder={handleAttachFromFinder}
              onAgentChange={(nextAgent) => {
                setSelectedAgent(nextAgent);
                setSaveAsRepoDefault(false);
                setShowSaveAsRepoDefault(nextAgent !== configuredDefaultAgent);
              }}
              onSaveAsRepoDefaultChange={setSaveAsRepoDefault}
              newWorkspace={
                offerNewWorkspace && !issue && newWorkspaceSetting !== undefined
                  ? {
                      checked: runInNewWorkspace,
                      onCheckedChange: setRunInNewWorkspace,
                    }
                  : undefined
              }
              onSubmit={handleSubmit}
              environment={environment}
            />
          </div>
        </div>
      </div>

      <FilePicker
        open={filePickerOpen}
        onOpenChange={setFilePickerOpen}
        repoPath={repoPath}
        workspaceId={workspaceId}
        workspacePath={workingDirectory}
        onFileSelect={handleFileSelect}
      />
    </>
  );
};
