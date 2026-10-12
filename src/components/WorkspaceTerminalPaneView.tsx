import React from "react";
import { ChevronDown, ChevronUp, Maximize2, Minimize2 } from "lucide-react";
import { type ConsolidatedTerminalHandle } from "./ConsolidatedTerminal";
import { Button } from "./ui/button";
import { Kbd, KbdGroup } from "./ui/kbd";
import { cn } from "../lib/utils";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "./ui/tooltip";
import { AgentTerminalPanel } from "./terminal/AgentTerminalPanel";
import { ResizeDivider } from "./terminal/ResizeDivider";
import { ShellTerminalPanel } from "./terminal/ShellTerminalPanel";
import { RemoteTerminalPanel } from "./RemoteTerminalPanel";
import { type TerminalWithWorkspace } from "./workspace-terminal-pane/types";
import { agentTerminalId } from "./terminal/agentTerminalId";
import { EnvironmentSelector } from "./EnvironmentSelector";
import type { RepoEnvironment } from "../hooks/useRepoEnvironment";

interface WorkspaceTerminalPaneViewProps {
  paneRef: React.RefObject<HTMLDivElement | null>;
  className?: string;
  collapsed: boolean;
  maximized: boolean;
  height: number;
  isResizingHeight: boolean;
  handleHeightResizeMouseDown: (e: React.MouseEvent) => void;
  totalTerminals: number;
  setCollapsed: React.Dispatch<React.SetStateAction<boolean>>;
  setMaximized: React.Dispatch<React.SetStateAction<boolean>>;
  scrollContainerRef: React.RefObject<HTMLDivElement | null>;
  terminals: TerminalWithWorkspace[];
  containerWidth: number;
  onNavigateToWorkspace?: (workspaceKey: string, isMainRepo: boolean) => void;
  remoteHost?: string;
  activePtySessionId: string | null;
  setActivePtySessionId: React.Dispatch<React.SetStateAction<string | null>>;
  handleCloseShell: (terminalId: string) => void | Promise<void>;
  onSessionError?: (message: string) => void;
  terminalRefs: React.RefObject<Map<string, ConsolidatedTerminalHandle | null>>;
  terminalWidths: Map<string, number | null>;
  handleTerminalResize: (
    leftId: string,
    rightId: string,
    deltaX: number,
  ) => void;
  handleCloseAgentSession: (sessionId: number) => void | Promise<void>;
  onTerminalDoubleClick: (terminalId: string) => void;
  onTerminalOutput?: (terminalId: string, fromProcess?: boolean) => void;
  onTerminalIdle?: (terminalId: string) => void;
  /** Where new terminals start; omitted while the devcontainers preview is off. */
  environment?: RepoEnvironment | null;
}

export const WorkspaceTerminalPaneView: React.FC<
  WorkspaceTerminalPaneViewProps
> = ({
  paneRef,
  className,
  collapsed,
  maximized,
  height,
  isResizingHeight,
  handleHeightResizeMouseDown,
  totalTerminals,
  setCollapsed,
  setMaximized,
  scrollContainerRef,
  terminals,
  containerWidth,
  onNavigateToWorkspace,
  remoteHost,
  activePtySessionId,
  setActivePtySessionId,
  handleCloseShell,
  onSessionError,
  terminalRefs,
  terminalWidths,
  handleTerminalResize,
  handleCloseAgentSession,
  onTerminalDoubleClick,
  onTerminalOutput,
  onTerminalIdle,
  environment,
}) => (
  <div
    ref={paneRef as React.Ref<HTMLDivElement>}
    className={className}
    style={{ display: "contents" }}
  >
    {!collapsed && !maximized && (
      <div
        className="relative flex-shrink-0 h-1 group"
        onMouseDown={handleHeightResizeMouseDown}
      >
        <div
          className={cn(
            "absolute inset-x-0 top-0 h-1 bg-border transition-colors",
            "group-hover:bg-primary/50",
            isResizingHeight && "bg-primary",
          )}
        />
        <div className="absolute inset-x-0 -top-1 h-3 cursor-ns-resize" />
      </div>
    )}

    <div
      data-testid="workspace-terminal-pane"
      className={cn(
        "relative flex flex-col flex-shrink-0",
        collapsed
          ? "self-end m-1.5 overflow-visible"
          : "border-t bg-background overflow-visible",
      )}
      style={
        collapsed
          ? undefined
          : {
              height: maximized ? "100%" : `${height}%`,
              maxHeight: maximized ? "100%" : "60%",
            }
      }
    >
      <TooltipProvider>
        <div
          data-testid="terminal-pane-controls"
          className={cn(
            "z-50 flex items-center gap-px rounded-md border border-border/80 bg-background/90 p-0.5 shadow-sm backdrop-blur-sm",
            // The controls sit above the pane's top edge; a maximized pane
            // has no room above it, so they move into its bottom corner.
            collapsed
              ? "contents"
              : maximized
                ? "absolute bottom-1.5 right-1.5"
                : "absolute -top-7 right-1.5",
          )}
        >
          {collapsed && (
            <PaneControlButton
              ariaLabel="Expand terminal"
              tooltip="Expand"
              shortcut="⌘ + J"
              onClick={() => setCollapsed(false)}
              className="absolute bottom-1.5 right-1.5"
            >
              <ChevronUp className="w-3 h-3" />
            </PaneControlButton>
          )}
          {!collapsed && environment && (
            <EnvironmentSelector
              env={environment}
              className="h-5 px-1.5 text-[11px] [&_svg]:h-3 [&_svg]:w-3"
            />
          )}
          {!collapsed && maximized && (
            <PaneControlButton
              ariaLabel="Restore terminal"
              tooltip="Restore"
              shortcut="⌘ + Ctrl + J"
              onClick={() => setMaximized(false)}
            >
              <Minimize2 className="w-3 h-3" />
            </PaneControlButton>
          )}
          {!collapsed && !maximized && totalTerminals > 0 && (
            <PaneControlButton
              ariaLabel="Maximize terminal"
              tooltip="Maximize"
              shortcut="⌘ + Ctrl + J"
              onClick={() => setMaximized(true)}
            >
              <Maximize2 className="w-3 h-3" />
            </PaneControlButton>
          )}
          {!collapsed && (
            <PaneControlButton
              ariaLabel="Collapse terminal"
              tooltip="Collapse"
              shortcut="⌘ + J"
              onClick={() => {
                setCollapsed(true);
                setMaximized(false);
              }}
            >
              <ChevronDown className="w-3 h-3" />
            </PaneControlButton>
          )}
        </div>
      </TooltipProvider>

      {!collapsed && (
        <div
          ref={scrollContainerRef as React.Ref<HTMLDivElement>}
          data-testid="terminal-scroll-container"
          className="flex-1 min-h-0 overflow-x-auto overflow-y-hidden flex"
          style={{ backgroundColor: "#1e1e1e" }}
          onWheelCapture={keepHorizontalWheelForPane}
        >
          {terminals.map(({ terminal, workspace }, index) => {
            const minTerminalPx = containerWidth * 0.4 || 300;
            const isLast = index === terminals.length - 1;
            const next = terminals[index + 1]?.terminal;
            const nextId = next
              ? next.type === "shell"
                ? next.data.id
                : agentTerminalId(next.data.sessionId)
              : null;
            const navigate = () =>
              onNavigateToWorkspace?.(
                workspace.workspaceKey,
                workspace.isMainRepo,
              );

            if (terminal.type === "shell" && terminal.data.remote) {
              const terminalId = terminal.data.id;
              const width = terminalWidths.get(terminalId);
              return (
                <React.Fragment key={terminalId}>
                  <div
                    data-terminal-id={terminalId}
                    data-remote-terminal="true"
                    className={cn(
                      "flex flex-col min-h-0 overflow-hidden flex-shrink-0 border-r border-border",
                      width == null && "flex-1",
                      activePtySessionId === terminalId &&
                        "ring-1 ring-inset ring-primary/40",
                    )}
                    style={{
                      width: width != null ? width : undefined,
                      minWidth: minTerminalPx,
                    }}
                    onMouseDown={() => setActivePtySessionId(terminalId)}
                    onDoubleClick={() => onTerminalDoubleClick(terminalId)}
                  >
                    <RemoteTerminalPanel
                      target={terminal.data.remote}
                      onClose={() => handleCloseShell(terminalId)}
                      onOutput={() => onTerminalOutput?.(terminalId, true)}
                    />
                  </div>
                  {!isLast && nextId && (
                    <ResizeDivider
                      onResize={(deltaX) =>
                        handleTerminalResize(terminalId, nextId, deltaX)
                      }
                    />
                  )}
                </React.Fragment>
              );
            }

            if (terminal.type === "shell") {
              const terminalId = terminal.data.id;
              return (
                <React.Fragment key={terminalId}>
                  <ShellTerminalPanel
                    terminalData={terminal.data}
                    remoteHost={terminal.data.remoteHost ?? remoteHost}
                    collapsed={collapsed}
                    isActive={activePtySessionId === terminalId}
                    onFocus={() => setActivePtySessionId(terminalId)}
                    onDoubleClick={() => onTerminalDoubleClick(terminalId)}
                    onClose={() => handleCloseShell(terminalId)}
                    canClose={true}
                    onSessionError={onSessionError}
                    onTerminalOutput={(_output, fromProcess) =>
                      onTerminalOutput?.(terminalId, fromProcess)
                    }
                    onTerminalIdle={() => onTerminalIdle?.(terminalId)}
                    terminalRefs={
                      terminalRefs as React.MutableRefObject<
                        Map<string, ConsolidatedTerminalHandle | null>
                      >
                    }
                    width={terminalWidths.get(terminalId)}
                    minWidth={minTerminalPx}
                    workspaceName={workspace.workspaceName}
                    onNavigateToWorkspace={navigate}
                  />
                  {!isLast && nextId && (
                    <ResizeDivider
                      onResize={(deltaX) =>
                        handleTerminalResize(terminalId, nextId, deltaX)
                      }
                    />
                  )}
                </React.Fragment>
              );
            }

            const terminalId = agentTerminalId(terminal.data.sessionId);
            const ptyId = terminal.data.ptySessionId;
            return (
              <React.Fragment key={terminalId}>
                <AgentTerminalPanel
                  sessionData={terminal.data}
                  remoteHost={remoteHost}
                  collapsed={collapsed}
                  isActive={activePtySessionId === ptyId}
                  onFocus={() => setActivePtySessionId(ptyId)}
                  onDoubleClick={() => onTerminalDoubleClick(terminalId)}
                  onClose={() =>
                    handleCloseAgentSession(terminal.data.sessionId)
                  }
                  onSessionError={onSessionError}
                  onTerminalOutput={(_output, fromProcess) =>
                    onTerminalOutput?.(terminalId, fromProcess)
                  }
                  onTerminalIdle={() => onTerminalIdle?.(terminalId)}
                  terminalRefs={
                    terminalRefs as React.MutableRefObject<
                      Map<string, ConsolidatedTerminalHandle | null>
                    >
                  }
                  width={terminalWidths.get(terminalId)}
                  minWidth={minTerminalPx}
                  workspaceName={workspace.workspaceName}
                  onNavigateToWorkspace={navigate}
                />
                {!isLast && nextId && (
                  <ResizeDivider
                    onResize={(deltaX) =>
                      handleTerminalResize(terminalId, nextId, deltaX)
                    }
                  />
                )}
              </React.Fragment>
            );
          })}
        </div>
      )}
    </div>
  </div>
);

// xterm cancels every wheel event while the running program has mouse
// reporting on (agent TUIs do), and cancels diagonal ones in the alt screen,
// which swallows horizontal trackpad swipes. Stop horizontal swipes before
// they reach xterm so the browser scrolls this container natively.
function keepHorizontalWheelForPane(e: React.WheelEvent) {
  if (Math.abs(e.deltaX) > Math.abs(e.deltaY)) e.stopPropagation();
}

function PaneControlButton({
  ariaLabel,
  tooltip,
  shortcut,
  onClick,
  className,
  children,
}: {
  ariaLabel: string;
  tooltip: string;
  shortcut: string;
  onClick: () => void;
  className?: string;
  children: React.ReactNode;
}) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button
          type="button"
          onClick={onClick}
          variant="ghost"
          className={cn("h-5 w-5 rounded-sm p-0", className)}
          aria-label={ariaLabel}
        >
          {children}
        </Button>
      </TooltipTrigger>
      <TooltipContent className="flex items-center gap-1.5">
        {tooltip}
        <KbdGroup>
          <Kbd>{shortcut}</Kbd>
        </KbdGroup>
      </TooltipContent>
    </Tooltip>
  );
}
