import { useEffect, useRef } from "react";
import useSWR from "swr";
import {
  type ActiveRepository,
  isRemoteRepository,
  repositoryCacheKey,
} from "../lib/active-repository";
import { scheduleRefreshWorkspaceChanges } from "../lib/change-file-drag";
import { transportChangeMarker } from "../lib/repository-adapter";
import { dispatch } from "../lib/remote-dispatch";
import { invalidateRemoteRepositoryData } from "../lib/remote-mutation-ui";
import { remoteActionKeys } from "../lib/remote-idempotency";

interface RemoteAgentStatus {
  running: boolean;
  should_refresh: boolean;
}

export function useRemoteAgentRefresh(
  repo: ActiveRepository | null,
  workspaceId: number | null,
) {
  const remoteRepo = isRemoteRepository(repo) ? repo : null;
  const cacheKey =
    remoteRepo && workspaceId != null && remoteRepo.endpoint
      ? ([
          "remote-agent-status",
          repositoryCacheKey(remoteRepo),
          workspaceId,
        ] as const)
      : null;
  const { data } = useSWR(
    cacheKey,
    async () => {
      if (!remoteRepo?.canonicalPath || workspaceId == null) {
        return { running: false, should_refresh: false };
      }
      return dispatch<RemoteAgentStatus>(remoteRepo.endpoint, {
        kind: "AgentStatus",
        repo: remoteRepo.canonicalPath,
        workspace: String(workspaceId),
      });
    },
    { refreshInterval: 5_000, dedupingInterval: 2_000 },
  );

  useEffect(() => {
    if (data?.should_refresh) invalidateRemoteRepositoryData();
  }, [data?.should_refresh]);
}

export function useRemoteChangeMarkerWatch(
  repo: ActiveRepository | null,
  workspaceId: number | null,
) {
  const remoteRepo = isRemoteRepository(repo) ? repo : null;
  const identity = remoteRepo ? repositoryCacheKey(remoteRepo) : "";
  const { data } = useSWR(
    remoteRepo
      ? (["remote-change-marker", identity, workspaceId] as const)
      : null,
    async () => {
      if (!remoteRepo) {
        return { operation_id: "" };
      }
      return transportChangeMarker(remoteRepo, workspaceId);
    },
    { refreshInterval: 4_000, dedupingInterval: 1_500 },
  );

  const lastSeenOperationId = useRef<string | null>(null);

  useEffect(() => {
    lastSeenOperationId.current = null;
  }, [identity, workspaceId]);

  useEffect(() => {
    const operationId = data?.operation_id;
    if (!operationId || !remoteRepo) return;
    if (lastSeenOperationId.current === null) {
      lastSeenOperationId.current = operationId;
      return;
    }
    if (operationId !== lastSeenOperationId.current) {
      lastSeenOperationId.current = operationId;
      // The repo changed and the view reloads with it, so a later identical
      // click is a new action rather than a retry of an uncertain one.
      remoteActionKeys.release(identity);
      invalidateRemoteRepositoryData();
      // The changes diff viewer and file browser load outside SWR. They
      // reload on the same event the local file watcher sends, scoped to the
      // workspace whose marker moved.
      scheduleRefreshWorkspaceChanges(
        workspaceId == null ? undefined : { workspaceId },
      );
    }
  }, [data?.operation_id, remoteRepo, workspaceId]);
}
