import { Search, X } from "lucide-react";
import React, {
  type KeyboardEvent as ReactKeyboardEvent,
  useEffect,
  useRef,
  useState,
} from "react";
import {
  ptyWrite,
  recordAgentChatScreen,
  recordAgentChatUserMessage,
  registerAgentChat,
} from "../../lib/api";
import { cn } from "../../lib/utils";
import {
  ConsolidatedTerminal,
  type ConsolidatedTerminalHandle,
} from "../ConsolidatedTerminal";
import { AgentIcon } from "../icons/AgentIcons";
import { Button } from "../ui/button";
import { Kbd, KbdGroup } from "../ui/kbd";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "../ui/tooltip";
import {
  type AgentChatRecorder,
  createAgentChatRecorder,
} from "./agentChatRecorder";
import { TerminalSearchOverlay } from "./TerminalSearchOverlay";
import { TerminalSendPreviews } from "./TerminalSendPreviews";
import { TerminalWorkspaceLabel } from "./TerminalWorkspaceLabel";
import { type AgentSessionData, dbSessionIdOf } from "./types";
import { useAgentAutoCommand } from "./useAgentAutoCommand";
import { agentTerminalId } from "./agentTerminalId";

export interface AgentTerminalPanelProps {
  sessionData: AgentSessionData;
  remoteHost?: string;
  collapsed: boolean;
  isActive?: boolean;
  onFocus?: () => void;
  onDoubleClick?: () => void;
  onClose?: () => void;
  onSessionError?: (message: string) => void;
  onTerminalOutput?: (output: string, fromProcess?: boolean) => void;
  onTerminalInput?: () => void;
  onTerminalIdle?: () => void;
  terminalRefs: React.MutableRefObject<
    Map<string, ConsolidatedTerminalHandle | null>
  >;
  width?: number | null;
  minWidth?: number;
  workspaceName: string;
  onNavigateToWorkspace?: () => void;
}

export const AgentTerminalPanel = ({
  sessionData,
  remoteHost,
  collapsed,
  isActive,
  onFocus,
  onDoubleClick,
  onClose,
  onSessionError,
  onTerminalOutput,
  onTerminalInput,
  onTerminalIdle,
  terminalRefs,
  width,
  minWidth,
  workspaceName,
  onNavigateToWorkspace,
}: AgentTerminalPanelProps) => {
  const searchInputRef = useRef<HTMLInputElement>(null);
  const [searchVisible, setSearchVisible] = useState(false);
  const [searchQuery, setSearchQuery] = useState("");
  const { isModelLoaded, autoCommand, prepareError } =
    useAgentAutoCommand(sessionData);

  const terminalId = agentTerminalId(sessionData.sessionId);
  const isHidden = collapsed;

  const chatRecorderRef = useRef<AgentChatRecorder | null>(null);

  useEffect(() => {
    const recorder = createAgentChatRecorder({
      register: () =>
        registerAgentChat(
          sessionData.repoPath,
          dbSessionIdOf(sessionData),
          sessionData.ptySessionId,
          sessionData.sessionName,
          sessionData.agent ?? "claude",
          null,
          sessionData.pendingPrompt,
        ),
      recordUserMessage: (screenBefore, text) =>
        recordAgentChatUserMessage(
          sessionData.repoPath,
          dbSessionIdOf(sessionData),
          screenBefore,
          text,
        ),
      recordScreen: (screen) =>
        recordAgentChatScreen(
          sessionData.repoPath,
          dbSessionIdOf(sessionData),
          screen,
        ),
      getScreen: () =>
        terminalRefs.current.get(terminalId)?.getScreenText() ?? "",
      onError: (error) => {
        console.error("Failed to capture agent chat log:", error);
      },
    });
    chatRecorderRef.current = recorder;
    return () => {
      recorder.dispose();
      if (chatRecorderRef.current === recorder) chatRecorderRef.current = null;
    };
  }, [
    sessionData.repoPath,
    dbSessionIdOf(sessionData),
    sessionData.ptySessionId,
    sessionData.sessionName,
    sessionData.agent,
    sessionData.pendingPrompt,
    terminalId,
    terminalRefs,
  ]);

  const writeAgentMessage = async (sessionId: string, data: string) => {
    const text = data.replace(/\r$/, "");
    await chatRecorderRef.current?.userMessage(text);
    await ptyWrite(sessionId, data);
  };

  const handleSendReview = (text: string) => {
    writeAgentMessage(sessionData.ptySessionId, `${text}\r`).catch(
      console.error,
    );
  };
  // Handle terminal output — agent is busy while streaming process output.
  const handleTerminalOutput = (output: string, fromProcess?: boolean) => {
    if (fromProcess !== false) {
      chatRecorderRef.current?.output();
    }
    onTerminalOutput?.(output, fromProcess);
  };

  const handleTerminalIdle = () => {
    chatRecorderRef.current?.idle();
    onTerminalIdle?.();
  };

  // Search handlers
  const openSearchPanel = () => {
    setSearchVisible(true);
    requestAnimationFrame(() => {
      searchInputRef.current?.focus();
      searchInputRef.current?.select();
    });
  };

  const closeSearchPanel = () => {
    setSearchVisible(false);
    setSearchQuery("");
    terminalRefs.current.get(terminalId)?.clearSearch();
  };

  const runSearch = (direction: "next" | "previous") => {
    if (!searchQuery.trim()) return;
    const terminal = terminalRefs.current.get(terminalId);
    if (!terminal) return;
    if (direction === "next") {
      terminal.findNext(searchQuery);
    } else {
      terminal.findPrevious(searchQuery);
    }
  };

  const handleSearchKeyDown = (e: ReactKeyboardEvent<HTMLInputElement>) => {
    if (e.key === "Enter") {
      e.preventDefault();
      if (e.shiftKey) {
        runSearch("previous");
      } else {
        runSearch("next");
      }
    } else if (e.key === "Escape") {
      closeSearchPanel();
    }
  };

  return (
    <div
      data-terminal-id={terminalId}
      data-active={isActive ? "true" : "false"}
      className={cn(
        "flex flex-col min-h-0 overflow-hidden flex-shrink-0",
        width == null && "flex-1",
      )}
      style={{
        width: width != null ? width : undefined,
        minWidth,
      }}
      onMouseDown={onFocus}
    >
      {/* Header */}
      <div
        className={cn(
          "h-7 min-h-[28px] flex items-center justify-between px-2 border-b border-r border-border flex-shrink-0",
          isActive ? "bg-primary/40" : "bg-gray-700",
        )}
        onDoubleClick={onDoubleClick}
      >
        <div className="flex items-center gap-1.5 min-w-0 text-sm font-medium text-gray-200">
          <AgentIcon
            agent={sessionData.agent}
            className="w-4 h-4 flex-shrink-0"
          />
          <span className="truncate">{sessionData.sessionName}</span>
          <TerminalWorkspaceLabel
            workspaceName={workspaceName}
            onNavigate={onNavigateToWorkspace}
            className="text-gray-300"
          />
        </div>
        <div className="flex items-center gap-1">
          {/* Search button */}
          <TooltipProvider>
            <Tooltip>
              <TooltipTrigger asChild>
                <Button
                  type="button"
                  onClick={openSearchPanel}
                  variant="ghost"
                  className="bg-transparent text-gray-200 hover:bg-muted/20 hover:text-gray-200"
                  size="xs"
                  aria-label="Search"
                >
                  <Search className="w-4 h-4" />
                </Button>
              </TooltipTrigger>
              <TooltipContent>Search (⌘+F)</TooltipContent>
            </Tooltip>
          </TooltipProvider>
          {/* Close button */}
          {onClose && (
            <TooltipProvider>
              <Tooltip>
                <TooltipTrigger asChild>
                  <Button
                    type="button"
                    onClick={onClose}
                    variant="ghost"
                    size="xs"
                    className="bg-transparent text-gray-200 hover:bg-muted/20 hover:text-gray-200"
                    aria-label="Close session"
                  >
                    <X className="w-4 h-4" />
                  </Button>
                </TooltipTrigger>
                <TooltipContent className="flex items-center gap-1.5">
                  Close
                  <KbdGroup>
                    <Kbd>⌘ + W</Kbd>
                  </KbdGroup>
                </TooltipContent>
              </Tooltip>
            </TooltipProvider>
          )}
        </div>
      </div>

      <div
        className="relative flex min-h-0 flex-1 flex-col overflow-hidden border-r border-border"
        style={{ backgroundColor: "#1e1e1e" }}
      >
        <div className="relative min-h-0 flex-1 overflow-hidden">
          {searchVisible && !collapsed && (
            <TerminalSearchOverlay
              searchInputRef={searchInputRef}
              searchQuery={searchQuery}
              onSearchQueryChange={setSearchQuery}
              onSearchKeyDown={handleSearchKeyDown}
              onFindPrevious={() => runSearch("previous")}
              onFindNext={() => runSearch("next")}
              onClose={closeSearchPanel}
            />
          )}

          {prepareError ? (
            <div className="flex flex-1 items-center justify-center px-4 text-center text-sm text-muted-foreground">
              Failed to start agent: {prepareError}
            </div>
          ) : isModelLoaded && autoCommand ? (
            <>
              <TerminalSendPreviews
                ptySessionId={sessionData.ptySessionId}
                isActive={!!isActive}
                onSendReview={handleSendReview}
                onInsertIntoTerminal={(text) => {
                  ptyWrite(sessionData.ptySessionId, text).catch(console.error);
                }}
              />
              <ConsolidatedTerminal
                key={sessionData.ptySessionId}
                ref={(el) => {
                  if (el) {
                    terminalRefs.current.set(terminalId, el);
                  } else {
                    terminalRefs.current.delete(terminalId);
                  }
                }}
                sessionId={sessionData.ptySessionId}
                workingDirectory={sessionData.workingDirectoryOverride}
                repoPath={sessionData.repoPath}
                workspaceId={sessionData.workspaceId}
                autoCommand={autoCommand}
                remoteHost={remoteHost}
                onSessionError={onSessionError}
                onClose={onClose}
                onTerminalOutput={handleTerminalOutput}
                onTerminalInput={onTerminalInput}
                onTerminalIdle={handleTerminalIdle}
                containerClassName="h-full w-full overflow-hidden"
                terminalPaneClassName="w-full h-full"
                isHidden={isHidden}
              />
            </>
          ) : (
            <div className="flex flex-1 items-center justify-center text-sm text-muted-foreground">
              Loading...
            </div>
          )}
        </div>
      </div>
    </div>
  );
};
