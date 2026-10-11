import type { AgentKind } from "./agents";
import {
  buildAgentAutoCommand,
  buildClaudeFilesystemSettings,
  buildTreqAgentSystemPrompt,
  claudeLocalSettingsPath,
  cursorPromptFileContents,
  mergeClaudeLocalSettings,
} from "./agentCommand";
import { cleanupAgentCliFiles, readFile, writeAgentCliFiles } from "./api";

export const parseJsonObject = (
  raw: string,
): Record<string, unknown> | null => {
  try {
    const parsed: unknown = JSON.parse(raw);
    if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
      return parsed as Record<string, unknown>;
    }
  } catch {
    return null;
  }
  return null;
};

export const prepareAgentAutoCommand = async ({
  agent,
  workspacePath,
  repoPath,
  supportingRepoPaths,
  sessionModel,
  permissionMode,
  pendingPrompt,
  treqBinDir,
}: {
  agent: AgentKind;
  workspacePath: string | null;
  repoPath: string;
  supportingRepoPaths?: string[];
  sessionModel: string | null;
  permissionMode?: string | null;
  pendingPrompt?: string | null;
  treqBinDir: string | null;
}): Promise<{
  command: string;
  filePaths: string[];
  skillWriteWarning?: string;
}> => {
  const cwd = workspacePath || repoPath;
  const agentPathContext = { workspacePath, repoPath, supportingRepoPaths };
  const systemPrompt = buildTreqAgentSystemPrompt(agentPathContext);

  let promptContents = systemPrompt;
  let settingsJson: string | undefined;
  if (agent === "cursor") {
    promptContents = cursorPromptFileContents(systemPrompt, pendingPrompt);
  } else if (agent === "claude") {
    let existing: Record<string, unknown> | null = null;
    try {
      existing = parseJsonObject(await readFile(claudeLocalSettingsPath(cwd)));
    } catch {
      existing = null;
    }
    settingsJson = JSON.stringify(
      mergeClaudeLocalSettings(
        existing,
        buildClaudeFilesystemSettings(agentPathContext),
      ),
      null,
      2,
    );
  }

  let skippedProjectSkills = false;
  const files = await writeAgentCliFiles(
    promptContents,
    settingsJson,
    cwd,
  ).catch((error) => {
    if (!cwd) {
      throw error;
    }
    skippedProjectSkills = true;
    return writeAgentCliFiles(promptContents, settingsJson, null);
  });
  const filePaths = [
    files.promptPath,
    files.settingsPath,
    files.skillDir,
    files.agentsSkillPath,
    files.claudeSkillPath,
  ].filter((path): path is string => !!path);

  const skillWriteWarning =
    files.skillWriteWarning ??
    (skippedProjectSkills
      ? "Could not write Treq skills into the workspace. The session still has the bundled copy."
      : undefined);

  return {
    command: buildAgentAutoCommand({
      agent,
      permissionMode,
      sessionModel,
      pendingPrompt: agent === "cursor" ? null : pendingPrompt,
      treqBinDir,
      files,
    }),
    filePaths,
    skillWriteWarning,
  };
};

export const discardAgentCliFiles = (paths: string[]): Promise<void> =>
  cleanupAgentCliFiles(paths);
