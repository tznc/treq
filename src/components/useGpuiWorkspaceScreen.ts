import { useRunningChecksOf } from "../hooks/useWorkspaceChecks";
import { DEFAULT_BROWSER_URL } from "./browser-panel/utils";
import type { JjLogCommit, Workspace } from "../lib/api";
import type { AgentReviewComment } from "../lib/api-types-review";
import type { ChangeFilesMoveRequest } from "../lib/change-file-drag";
import type { ParsedFileChange } from "../lib/git-utils";
import type { SessionCreationInfo } from "../types/sessions";
import type { useGpuiPartsHost } from "../lib/gpui-parts";
import {
  type GpuiScreenAction,
  type GpuiWorkspaceScreen,
  type GpuiWorkspaceTab,
  useGpuiScreen,
} from "../lib/gpui-screen";
import { popupNativeMenu } from "../lib/native-context-menu";
import { useCacheRevision } from "../lib/swr-cache";

/** The header controls whose React flows GPUI still asks for (`useGpuiPart`). */
type PartControl =
  | "target"
  | "viewPr"
  | "createPr"
  | "checks"
  | "mergeQueue"
  | "browser";

type Route = Omit<
  GpuiWorkspaceScreen,
  | "kind"
  | "browser"
  | "creatingPr"
  | "runningChecks"
  | "runningWorkflows"
  | "setupRerunning"
  | "mergeQueue"
  | "revision"
> & {
  /** The Browser view shows; its url comes from `BrowserPanel`'s part. */
  showBrowser: boolean;
};

interface Handlers {
  setTab: (tab: GpuiWorkspaceTab) => void;
  /** The Code tab's height, which GPUI frames the terminal pane below. */
  contentHeight?: (height: number) => void;
  /** The Code tab's file list: open a file or folder in the file browser. */
  openEntry?: (entry: { name: string; isDirectory: boolean }) => void;
  openFilePicker?: () => void;
  /** The Code tab's commit history: show the commit in the Commits tab. */
  openCommit?: (changeId: string) => void;
  /** The Code tab's stack: go to another workspace. */
  navigateToWorkspace?: (workspace: Workspace) => void;
  /** The Code or Commits tab started an agent session. */
  sessionCreated?: (session: SessionCreationInfo) => void;
  /** The Commits tab showed the commit `openCommit` asked for. */
  commitScrolled?: () => void;
  /** The Commits tab: move a commit to a new or an existing workspace. */
  moveCommitToNew?: (commit: JjLogCommit) => void;
  moveCommitToExisting?: (commit: JjLogCommit) => void;
  /** The Commits tab stashed a commit. */
  commitStashed?: () => void;
  /** The Changes tab read its files, or started or finished reading them. */
  changedFiles?: (files: ParsedFileChange[]) => void;
  changesRefreshing?: (refreshing: boolean) => void;
  /** The Changes tab: confirm moving files, or move them to a new workspace. */
  moveFiles?: (request: ChangeFilesMoveRequest) => void;
  moveFilesToNew?: (files: string[]) => void;
  showCommitted?: (show: boolean) => void;
  /** The Changes tab's Start Review: a review agent with `prompt`. */
  startReview?: (prompt: string, agent?: string) => void;
  /** An agent review comment's "Send to agent". */
  sendAgentComment?: (comment: AgentReviewComment) => void;
  /** A finished review: an agent works through `markdown`. */
  sendReview?: (
    markdown: string,
    mode: "plan" | "acceptEdits",
    useIdleAgent: boolean,
  ) => void;
  /** The Logs tab's "Send to agent": an agent works through `prompt`. */
  sendLogsToAgent?: (prompt: string) => void;
  /** GPUI's terminal pane: a `terminal.*` field. */
  terminal?: (field: string, value: unknown) => void;
  /** The Diff/Browser switcher. */
  reviewSubView?: (view: "diff" | "browser") => void;
  openBranchSwitcher?: () => void;
  stack?: () => void;
  merge?: () => void;
  /** The target branch popover picked a branch: retarget the workspace. */
  selectTarget?: (branch: string) => void;
  /** The Details popover's "View full prompt". */
  viewFullPrompt?: (promptId: number) => void;
  deleteWorkspace?: () => void;
}

const TABS: GpuiWorkspaceTab[] = ["overview", "commits", "changes", "logs"];

interface PartState {
  creating?: boolean;
}

/**
 * Draws ShowWorkspace's header and tab row in GPUI while `active`, with the
 * webview framed below them. GPUI reads them from the backend and pushes,
 * syncs and rebases itself; React sends which workspace shows and the state
 * of the flows it still runs. Returns whether GPUI draws them, so React
 * leaves its own out of the frame.
 *
 * The popovers under the header's target branch, checks and details buttons
 * are GPUI's too: they open in its modal window, so the screen stays. What
 * only React can do goes back here: retargeting, running and trusting
 * workflow checks (`useWorkspaceChecks`), the prompt history.
 */
export function useGpuiWorkspaceScreen({
  active,
  parts,
  route,
  handlers,
}: {
  active: boolean;
  parts: ReturnType<typeof useGpuiPartsHost>;
  route: Route;
  handlers: Handlers;
}): boolean {
  const revision = useCacheRevision(active);
  const running = useRunningChecksOf(route.workspaceId);
  const part = (key: PartControl) =>
    (parts.parts[key] as PartState | null | undefined) ?? null;

  const { showBrowser, ...rest } = route;
  const view: GpuiWorkspaceScreen = {
    kind: "workspace",
    ...rest,
    browser: showBrowser
      ? {
          url:
            (parts.parts.browser as { url?: string } | null | undefined)?.url ??
            DEFAULT_BROWSER_URL,
        }
      : null,
    creatingPr: !!part("createPr")?.creating,
    runningChecks: running.files,
    runningWorkflows: running.workflows,
    setupRerunning: running.setup,
    mergeQueue:
      (parts.parts.mergeQueue as GpuiWorkspaceScreen["mergeQueue"]) ?? null,
    revision,
  };

  const call = (key: PartControl, name: string, ...args: unknown[]) =>
    (parts.handlers(key)?.[name] as ((...a: unknown[]) => void) | undefined)?.(
      ...args,
    );

  const press = (id: string) => {
    switch (id) {
      case "workspace.branchSwitcher":
        return handlers.openBranchSwitcher?.();
      case "workspace.stack":
        return handlers.stack?.();
      case "workspace.viewPr":
        return call("viewPr", "view");
      case "workspace.createPr":
        return call("createPr", "create");
      case "workspace.createPrMenu":
        return call("createPr", "menu");
      case "workspace.checksRunAll":
        return call("checks", "runAll");
      case "workspace.checksTrust":
        return call("checks", "trust");
      case "workspace.checksRerunSetup":
        return call("checks", "rerunSetup");
      case "workspace.checksLogs":
        return call("checks", "viewLogs");
      case "browser.reload":
        return call("browser", "reload");
      case "workspace.mergeQueue":
        return call("mergeQueue", "press");
      case "workspace.merge":
        return handlers.merge?.();
      case "code.filePicker":
        return handlers.openFilePicker?.();
      case "workspace.more": {
        const remove = handlers.deleteWorkspace;
        if (!remove) return;
        return void popupNativeMenu([
          {
            kind: "item",
            text: "Delete Workspace",
            icon: "trash-2",
            destructive: true,
            onSelect: remove,
          },
        ]);
      }
      default:
    }
  };

  const field = (name: string, value: unknown) => {
    switch (name) {
      case "workspace.tab":
        if (TABS.includes(value as GpuiWorkspaceTab)) {
          handlers.setTab(value as GpuiWorkspaceTab);
        }
        return;
      case "workspace.target":
        return handlers.selectTarget?.(value as string);
      case "workspace.viewFullPrompt":
        return handlers.viewFullPrompt?.(value as number);
      case "browser.navigate":
        return call("browser", "navigate", value);
      case "workspace.checksRun":
        return call("checks", "run", value);
      case "workspace.contentHeight":
        return handlers.contentHeight?.(value as number);
      case "code.open":
        return handlers.openEntry?.(
          value as { name: string; isDirectory: boolean },
        );
      case "code.commit":
        return handlers.openCommit?.(value as string);
      case "code.stack":
        return handlers.navigateToWorkspace?.(value as Workspace);
      case "code.sessionCreated":
        return handlers.sessionCreated?.(value as SessionCreationInfo);
      case "commits.scrolled":
        return handlers.commitScrolled?.();
      case "commits.moveToNew":
        return handlers.moveCommitToNew?.(value as JjLogCommit);
      case "commits.moveToExisting":
        return handlers.moveCommitToExisting?.(value as JjLogCommit);
      case "commits.stashed":
        return handlers.commitStashed?.();
      case "changes.files":
        return handlers.changedFiles?.(value as ParsedFileChange[]);
      case "changes.refreshing":
        return handlers.changesRefreshing?.(value as boolean);
      case "changes.moveFiles":
        return handlers.moveFiles?.(value as ChangeFilesMoveRequest);
      case "changes.moveToNew":
        return handlers.moveFilesToNew?.(value as string[]);
      case "changes.showCommitted":
        return handlers.showCommitted?.(value as boolean);
      case "changes.startReview": {
        const { prompt, agent } = value as { prompt: string; agent?: string };
        return handlers.startReview?.(prompt, agent ?? undefined);
      }
      case "changes.agentComment":
        return handlers.sendAgentComment?.(value as AgentReviewComment);
      case "changes.review": {
        const { markdown, mode, useIdleAgent } = value as {
          markdown: string;
          mode: "plan" | "acceptEdits";
          useIdleAgent: boolean;
        };
        return handlers.sendReview?.(markdown, mode, useIdleAgent);
      }
      case "logs.sendToAgent":
        return handlers.sendLogsToAgent?.(value as string);
      case "terminal.activeSession":
      case "terminal.closeSession":
      case "terminal.newAgent":
      case "terminal.navigate":
      case "terminal.summaries":
      case "terminal.dismissAsset":
      case "terminal.shortcut":
        return handlers.terminal?.(name, value);
      case "changes.subView":
        if (value === "diff" || value === "browser") {
          handlers.reviewSubView?.(value);
        }
        return;
      default:
    }
  };

  return useGpuiScreen(active ? view : null, (action: GpuiScreenAction) => {
    if (!active) return;
    switch (action.kind) {
      case "press":
        press(action.id);
        break;
      case "field":
        field(action.field, action.value);
        break;
      default:
    }
  });
}
