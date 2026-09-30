import { useEffect, useState } from "react";
import useSWR from "swr";
import {
  getSessionModel,
  getTreqBinDir,
  listSupportingRepos,
} from "../../lib/api";
import { prepareAgentAutoCommand } from "../../lib/prepareAgentAutoCommand";
import { useToast } from "../ui/toast";
import type { AgentSessionData } from "./types";

// `restarted` is set once the agent has been relaunched in place (after a
// model change). The relaunch starts a fresh agent without the session's
// original task, which the first launch already ran.
export const useAgentAutoCommand = (
  sessionData: AgentSessionData,
  restarted = false,
) => {
  const { addToast } = useToast();
  const [sessionModelOverride, setSessionModelState] = useState<
    string | null | undefined
  >(undefined);

  const { data: loadedModel, isLoading: modelLoading } = useSWR(
    ["session-model", sessionData.repoPath, sessionData.sessionId],
    () => getSessionModel(sessionData.repoPath, sessionData.sessionId),
  );
  const { data: treqBinDir = null, isLoading: binLoading } = useSWR(
    "treq-bin-dir",
    getTreqBinDir,
  );

  const { data: supportingRepos, isLoading: supportingLoading } = useSWR(
    ["supporting-repos", sessionData.repoPath],
    () => listSupportingRepos(sessionData.repoPath),
  );
  const supportingRepoPaths = (supportingRepos ?? [])
    .filter((repo) => repo.exists)
    .map((repo) => repo.path);

  const sessionModel =
    sessionModelOverride === undefined
      ? (loadedModel ?? null)
      : sessionModelOverride;
  const isModelLoaded = !modelLoading;
  const treqBinDirReady = !binLoading;

  const pendingPrompt = restarted ? undefined : sessionData.pendingPrompt;
  const { data: prepared, error: prepareErr } = useSWR(
    isModelLoaded && treqBinDirReady && !supportingLoading
      ? [
          "agent-auto-command",
          sessionData.agent ?? "claude",
          sessionData.workspacePath,
          sessionData.repoPath,
          supportingRepoPaths.join("\n"),
          sessionModel,
          treqBinDir,
          pendingPrompt,
          sessionData.permissionMode,
        ]
      : null,
    () =>
      prepareAgentAutoCommand({
        agent: sessionData.agent ?? "claude",
        workspacePath: sessionData.workspacePath,
        repoPath: sessionData.repoPath,
        supportingRepoPaths,
        sessionModel,
        permissionMode: sessionData.permissionMode,
        pendingPrompt,
        treqBinDir,
      }),
  );

  const [prepareError, setPrepareError] = useState<string | null>(null);

  useEffect(() => {
    if (prepareErr) {
      setPrepareError(
        prepareErr instanceof Error ? prepareErr.message : String(prepareErr),
      );
    } else {
      setPrepareError(null);
    }
    if (prepared?.skillWriteWarning) {
      addToast({
        type: "warning",
        title: "Could not write Treq skills",
        description: prepared.skillWriteWarning,
      });
    }
  }, [prepareErr, prepared?.skillWriteWarning, addToast]);

  return {
    sessionModel,
    setSessionModelState,
    isModelLoaded,
    autoCommand: prepared?.command ?? null,
    prepareError,
  };
};
