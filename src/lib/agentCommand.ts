import type { AgentKind } from "./agents";
import { resolveCommit } from "./api";
import { shellQuote } from "./shellQuote";

/** Append an agent prompt as a positional argument, never as a CLI option. */
export const appendAgentPrompt = (command: string, prompt: string): string =>
  `${command} -- ${shellQuote(prompt)}`;

interface AgentPathContext {
  workspacePath: string | null;
  repoPath: string;
  /** Supporting repositories of the main repository; readable, never written. */
  supportingRepoPaths?: string[];
}

export interface AgentCliFiles {
  promptPath: string;
  settingsPath?: string;
  skillDir?: string;
  agentsSkillPath?: string;
  claudeSkillPath?: string;
}

/** Build the single-line system prompt injected into agent terminal sessions. */
export const buildTreqAgentSystemPrompt = ({
  workspacePath,
  repoPath,
  supportingRepoPaths = [],
}: AgentPathContext): string => {
  if (typeof resolveCommit !== "function") {
    throw new Error("resolveCommit Tauri invoke wrapper is missing");
  }

  const locationContext = workspacePath
    ? [
        "You are operating inside a Treq workspace.",
        `Your current working directory and direct filesystem scope is ${workspacePath}.`,
        `The Treq home repository is ${repoPath}.`,
        "The home repository and sibling workspaces are outside your direct filesystem scope.",
      ]
    : [
        "You are operating in the Treq home repository.",
        `Your current working directory and direct filesystem scope is ${repoPath}.`,
      ];

  return [
    ...locationContext,
    "You may freely read and write files within this directory and its descendants.",
    "Do not directly read, write, edit, or delete files outside this directory.",
    "A Treq skill named treq is loaded automatically for this session.",
    "Follow it for the treq CLI, workspaces, sending files to the user, and commits.",
    "You may run treq CLI commands even when they create or manage workspaces outside the current working directory.",
    ...(supportingRepoPaths.length > 0
      ? [
          `Supporting repositories: ${supportingRepoPaths.join(", ")}.`,
          "You may read files in supporting repositories but must not write to them directly.",
          'To change a supporting repository, create a workspace in it with `treq add -r <repo> <branch>` and start a scoped agent there with `treq agent -r <repo> <branch> "<prompt>"`.',
          "Check its progress with `treq st -r <repo> <branch>`.",
        ]
      : []),
  ].join(" ");
};

/** Workspace/home filesystem allow and deny lists for Claude `--settings`. */
export const buildClaudeFilesystemSettings = ({
  workspacePath,
  repoPath,
  supportingRepoPaths = [],
}: AgentPathContext) => ({
  filesystem: workspacePath
    ? {
        denyRead: [repoPath],
        allowRead: [workspacePath, ...supportingRepoPaths],
        allowWrite: [workspacePath],
      }
    : {
        allowRead: [repoPath, ...supportingRepoPaths],
        allowWrite: [repoPath],
      },
});

/** Overlay Treq filesystem restrictions onto `.claude/settings.local.json`, without sandbox. */
export const mergeClaudeLocalSettings = (
  existing: Record<string, unknown> | null,
  filesystemSettings: ReturnType<typeof buildClaudeFilesystemSettings>,
): Record<string, unknown> => {
  const rest = { ...existing };
  delete rest.sandbox;
  return {
    ...rest,
    filesystem: filesystemSettings.filesystem,
  };
};

export const claudeLocalSettingsPath = (cwd: string): string =>
  `${cwd.replace(/\/+$/, "")}/.claude/settings.local.json`;

const shellCat = (path: string): string => `$(cat -- ${shellQuote(path)})`;

const wrapWithTempFileCleanup = (command: string, paths: string[]): string => {
  const quoted = paths.map(shellQuote).join(" ");
  return `( trap "rm -rf ${quoted}" EXIT; ${command} )`;
};

// Runs in the terminal's shell once the agent subshell ends, so the app can
// notify "Agent finished". A status of 128 or more means a signal killed the
// agent, which is how Treq closes or restarts a session, so it stays quiet.
const NOTIFY_ON_AGENT_EXIT =
  "[ $? -lt 128 ] && treq notify --agent-exited >/dev/null 2>&1";

export const cursorPromptFileContents = (
  systemPrompt: string,
  pendingPrompt?: string | null,
): string =>
  pendingPrompt ? `${systemPrompt} ${pendingPrompt}` : systemPrompt;

export interface BuildAgentAutoCommandOptions {
  agent: AgentKind;
  permissionMode?: string | null;
  sessionModel?: string | null;
  pendingPrompt?: string | null;
  treqBinDir?: string | null;
  files: AgentCliFiles;
}

/**
 * Build the PTY auto-command for an agent CLI.
 *
 * Long prompt/settings bodies are never inlined. Claude reads files via native
 * flags; Codex and Cursor have no append-from-file flags, so the shell expands
 * `$(cat -- path)` at exec time (the typed command stays short).
 */
export const buildAgentAutoCommand = ({
  agent,
  permissionMode,
  sessionModel,
  pendingPrompt,
  treqBinDir,
  files,
}: BuildAgentAutoCommandOptions): string => {
  let autoCommand: string;
  const cleanupPaths = [files.promptPath];
  if (files.skillDir) {
    cleanupPaths.push(files.skillDir);
  }
  if (files.agentsSkillPath) {
    cleanupPaths.push(files.agentsSkillPath);
  }
  if (files.claudeSkillPath) {
    cleanupPaths.push(files.claudeSkillPath);
  }

  if (agent === "codex") {
    autoCommand = `codex -c "developer_instructions=${shellCat(files.promptPath)}"`;
    if (pendingPrompt) {
      autoCommand = appendAgentPrompt(autoCommand, pendingPrompt);
    }
  } else if (agent === "cursor") {
    const planFlag = permissionMode === "plan" ? " --plan" : "";
    autoCommand = `cursor-agent${planFlag} -- "${shellCat(files.promptPath)}"`;
  } else if (agent === "copilot") {
    autoCommand = `copilot --allow-all-tools -- "${shellCat(files.promptPath)}"`;
    if (pendingPrompt) {
      autoCommand = appendAgentPrompt(autoCommand, pendingPrompt);
    }
  } else {
    const permissionModeArg =
      permissionMode === "plan"
        ? " --permission-mode plan"
        : " --permission-mode acceptEdits";
    autoCommand = `claude${permissionModeArg}`;
    if (sessionModel) {
      autoCommand += ` --model=${shellQuote(sessionModel)}`;
    }
    if (!files.settingsPath) {
      throw new Error("Claude auto-command requires a settings file path");
    }
    cleanupPaths.push(files.settingsPath);
    autoCommand += ` --settings ${shellQuote(files.settingsPath)}`;
    autoCommand += ` --append-system-prompt-file ${shellQuote(files.promptPath)}`;
    if (pendingPrompt) {
      autoCommand += ` -- ${shellQuote(pendingPrompt)}`;
    }
  }

  autoCommand = `${wrapWithTempFileCleanup(autoCommand, cleanupPaths)}; ${NOTIFY_ON_AGENT_EXIT}`;

  if (treqBinDir) {
    autoCommand = `export PATH=${shellQuote(treqBinDir)}:$PATH; ${autoCommand}`;
  }

  return autoCommand;
};
