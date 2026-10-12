import React, { useEffect, useRef, useState } from "react";
import {
  getWorkspaceChangedFiles,
  getWorkspaceDiff,
  getWorkspaceFileHunksBatch,
  type WorkspaceFileHunksBatchFile,
} from "../../../lib/api";
import {
  type ParsedFileChange,
  parseJjChangedFiles,
} from "../../../lib/git-utils";
import { useCachedWorkspaceChanges } from "../../../hooks/useCachedWorkspaceChanges";
import {
  REFRESH_WORKSPACE_CHANGES_EVENT,
  scheduleRefreshWorkspaceChanges,
} from "../../../lib/change-file-drag";
import type { WorkspaceChangesRefreshDetail } from "../../../lib/workspace-refresh";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { useToast } from "../../ui/toast";
import type { FileHunksData } from "../types";
import { workspaceDiffCoalesce } from "../../../lib/coalesce-in-flight";
import {
  applyHunkBatch,
  chunkPaths,
  replaceGeneration,
} from "./diffLoadingCoordinator";

// One in-flight workspace_diff per process. jj WC locks are process-global.

interface UseFileLoadingParams {
  workspacePath: string;
  repoPath: string | undefined;
  workspaceId: number | undefined;
  isHomeRepo: boolean;
  showCommittedChanges: boolean | undefined;
  /** Backend conflicted-file hint — retained even when Committed is hidden. */
  conflictedFilesHint?: string[];
  onRefreshingChange: ((refreshing: boolean) => void) | undefined;
  /** Ref updated each render with current setLargeChangesetExpanded (breaks ordering dep) */
  setLargeChangesetExpandedRef: React.MutableRefObject<
    React.Dispatch<React.SetStateAction<boolean>>
  >;
  /** Ref updated each render with current applyChangedFiles (breaks circular dep) */
  applyChangedFilesRef: React.MutableRefObject<
    (parsed: ParsedFileChange[], forceApply?: boolean) => void
  >;
  /** Ref updated each render with current isInReviewMode (breaks circular dep) */
  isInReviewModeRef: React.MutableRefObject<boolean>;
  /** Ref updated each render with current setStaleFiles (breaks circular dep) */
  setStaleFilesRef: React.MutableRefObject<
    React.Dispatch<React.SetStateAction<Set<string>>>
  >;
  /** Ref updated each render with current setPendingHunksData (breaks circular dep) */
  setPendingHunksDataRef: React.MutableRefObject<
    React.Dispatch<React.SetStateAction<Map<string, FileHunksData> | null>>
  >;
  isReloadingRef: React.MutableRefObject<boolean>;
  addToast: ReturnType<typeof useToast>["addToast"];
}

export function useFileLoading({
  workspacePath,
  repoPath,
  workspaceId,
  isHomeRepo,
  showCommittedChanges,
  conflictedFilesHint = [],
  onRefreshingChange,
  setLargeChangesetExpandedRef,
  applyChangedFilesRef,
  isInReviewModeRef,
  setStaleFilesRef,
  setPendingHunksDataRef,
  isReloadingRef,
  addToast,
}: UseFileLoadingParams) {
  const [files, setFiles] = useState<ParsedFileChange[]>([]);
  const [allFileHunks, setAllFileHunks] = useState<Map<string, FileHunksData>>(
    new Map(),
  );
  const [loadingAllHunks, setLoadingAllHunks] = useState(false);
  const [initialLoading, setInitialLoading] = useState(true);
  const [, setRefreshing] = useState(false);
  const [committedFiles, setCommittedFiles] = useState<
    import("../../../lib/api").JjFileChange[]
  >([]);
  const [committedFileHunks, setCommittedFileHunks] = useState<
    Map<string, FileHunksData>
  >(new Map());
  // null until the first getWorkspaceDiff resolves — fall back to status hint.
  // After that, diff.conflicted_files is authoritative for Review Conflicts UI
  // so marker resolves clear before a stale workspace-status query refetches.
  const [liveConflictedFiles, setLiveConflictedFiles] = useState<
    string[] | null
  >(null);
  const prevFilePathsRef = useRef<string[]>([]);
  // Stabilize against default `[]` / new array identity each render — a changing
  // loadChangedFiles identity would retrigger the showCommittedChanges effect
  // in a loop and unmount the Review tree (empty <body /> in unit tests).
  const conflictedFilesHintRef = useRef(conflictedFilesHint);
  conflictedFilesHintRef.current = conflictedFilesHint;
  const conflictedFilesKey = conflictedFilesHint.join("\0");

  const cachedChanges = useCachedWorkspaceChanges(workspacePath, {
    enabled: true,
    repoPath: workspacePath,
    workspaceId: null,
  });

  const invalidateCache = async () => {
    await cachedChanges.refresh();
  };

  const refresh = () => {
    cachedChanges.refresh();
  };

  const pendingForceApplyRef = useRef(false);
  const hunkGenerationRef = useRef(0);
  const snapshotTokenRef = useRef<string | null>(null);
  const preloadCancelRef = useRef<(() => void) | null>(null);

  const loadChangedFiles = async (forceApply = false) => {
    if (forceApply) pendingForceApplyRef.current = true;
    await workspaceDiffCoalesce(async () => {
      const force = pendingForceApplyRef.current;
      pendingForceApplyRef.current = false;
      setRefreshing(true);
      onRefreshingChange?.(true);
      // User-initiated refreshes (e.g. commit) must apply through review-mode
      // freeze so the Review panel updates instead of showing the stale banner.
      if (force) {
        isReloadingRef.current = true;
      }
      try {
        if (repoPath && workspaceId !== undefined) {
          const diff = await getWorkspaceDiff(repoPath, workspaceId);
          const parsed = parseJjChangedFiles(diff.uncommitted_files ?? []);
          applyChangedFilesRef.current(parsed, force);

          const fromDiff = diff.conflicted_files ?? [];
          // Diff is authoritative for live conflict state. The status hint can
          // lag a frame behind resolve+commit; never re-introduce paths a fresh
          // diff reports as resolved.
          setLiveConflictedFiles(fromDiff);
          const conflictedHint = new Set<string>(fromDiff);
          const uncommittedPaths = new Set(parsed.map((file) => file.path));

          // Keep the full committed file list so the Committed section header
          // (and its Show toggle) stay available while committed diffs are hidden.
          // Conflicted paths that aren't already in the committed list are still
          // appended — rebase conflicts live in committed hunks.
          let committed = [...(diff.committed_files ?? [])];
          for (const path of conflictedHint) {
            if (
              uncommittedPaths.has(path) ||
              committed.some((file) => file.path === path)
            ) {
              continue;
            }
            committed = [
              ...committed,
              {
                path,
                status: "C",
                previous_path: null,
                changed_line_count: 0,
                diff_deferred: false,
              },
            ];
          }
          setCommittedFiles(committed);

          // When Committed is hidden, still keep dirty + conflicted committed hunks.
          const alwaysVisibleCommitted = new Set([
            ...uncommittedPaths,
            ...conflictedHint,
          ]);
          setCommittedFileHunks(
            new Map(
              (diff.hunks_by_file ?? [])
                .filter(
                  (fileDiff) =>
                    showCommittedChanges ||
                    alwaysVisibleCommitted.has(fileDiff.path),
                )
                .map((fileDiff) => [
                  fileDiff.path,
                  {
                    filePath: fileDiff.path,
                    hunks: fileDiff.hunks,
                    isLoading: false,
                  },
                ]),
            ),
          );
          return;
        }
        const jjFiles = await getWorkspaceChangedFiles(
          repoPath ?? "",
          workspaceId ?? null,
        );
        const parsed = parseJjChangedFiles(jjFiles);
        applyChangedFilesRef.current(parsed, force);
        setCommittedFiles([]);
        setCommittedFileHunks(new Map());
        setLiveConflictedFiles([]);
      } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        addToast({ description: message, title: "JJ Error", type: "error" });
      } finally {
        setInitialLoading(false);
        setRefreshing(false);
        onRefreshingChange?.(false);
        if (force) {
          // Keep the flag long enough for the files→hunks effect to apply.
          setTimeout(() => {
            isReloadingRef.current = false;
          }, 100);
        }
      }
    });
  };

  const loadChangedFilesRef = useRef(loadChangedFiles);
  loadChangedFilesRef.current = loadChangedFiles;

  // Call through the ref so toast/callback identity cannot retrigger this
  // effect. Including `loadChangedFiles` stacked overlapping getWorkspaceDiff
  // calls (jj WC lock) and unmounted the Review file list in later tests.
  // A regular workspace must wait for its database id. The home repo has no
  // workspace id by design and loads through the optional-id changed-files API.
  useEffect(() => {
    if (!workspacePath || (!isHomeRepo && workspaceId === undefined)) return;
    void loadChangedFilesRef.current();
  }, [
    workspacePath,
    repoPath,
    workspaceId,
    isHomeRepo,
    showCommittedChanges,
    conflictedFilesKey,
  ]);

  useEffect(() => {
    if (!workspaceId) return;
    const unlisten = listen<{ workspace_id: number; changed_paths: string[] }>(
      "workspace-files-changed",
      (event) => {
        if (event.payload.workspace_id === workspaceId) {
          scheduleRefreshWorkspaceChanges({
            workspaceId,
            changedPaths: event.payload.changed_paths,
          });
        }
      },
    );
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [workspaceId]);

  const refreshCommittedChanges = async () => {};

  const loadAllFileHunks = async (
    filesToLoad: ParsedFileChange[],
    forceApply = false,
  ) => {
    if (filesToLoad.length === 0) {
      if (!isInReviewModeRef.current || forceApply) {
        setAllFileHunks((prev) => (prev.size === 0 ? prev : new Map()));
      }
      setLoadingAllHunks(false);
      return;
    }
    const generation = ++hunkGenerationRef.current;
    preloadCancelRef.current?.();
    snapshotTokenRef.current = null;
    setLoadingAllHunks(true);
    const paths = filesToLoad.map((file) => file.path);
    if (!isInReviewModeRef.current || forceApply) {
      setAllFileHunks((prev) => replaceGeneration(prev, paths));
    }
    try {
      // Load the first couple of files immediately so the Changes tab shows
      // real content right away, then chunk the rest in the background.
      const priorityPaths = paths.slice(0, 2);
      const remainingPaths = paths.slice(2);
      const batches = [
        ...(priorityPaths.length > 0 ? [priorityPaths] : []),
        ...chunkPaths(remainingPaths, 32),
      ];
      const results: WorkspaceFileHunksBatchFile[] = [];
      const loadBatch = async (index: number): Promise<void> => {
        const batch = batches[index];
        if (!batch) return;
        if (index > 0) {
          await new Promise<void>((resolve) => {
            let cancelled = false;
            const callback = () => {
              if (!cancelled) resolve();
            };
            const idle = window.requestIdleCallback?.(callback, {
              timeout: 100,
            });
            const timer =
              idle === undefined ? window.setTimeout(callback, 25) : undefined;
            preloadCancelRef.current = () => {
              cancelled = true;
              if (idle !== undefined) window.cancelIdleCallback?.(idle);
              if (timer !== undefined) window.clearTimeout(timer);
              resolve();
            };
          });
        }
        if (generation !== hunkGenerationRef.current) return;
        const response = await getWorkspaceFileHunksBatch(
          repoPath ?? "",
          workspaceId ?? null,
          batch,
          snapshotTokenRef.current ?? undefined,
        );
        if (generation !== hunkGenerationRef.current) return;
        // Test doubles and older bridges can omit a newly-added command while
        // the workspace metadata path remains usable.
        if (!response) return loadBatch(index + 1);
        if (
          snapshotTokenRef.current &&
          snapshotTokenRef.current !== response.snapshotToken
        ) {
          void loadChangedFilesRef.current();
          return;
        }
        snapshotTokenRef.current = response.snapshotToken;
        results.push(...response.files);
        if (
          !isInReviewModeRef.current ||
          forceApply ||
          isReloadingRef.current
        ) {
          setAllFileHunks((prev) => applyHunkBatch(prev, response.files));
        }
        await loadBatch(index + 1);
      };
      await loadBatch(0);
      if (isInReviewModeRef.current && !forceApply && !isReloadingRef.current) {
        const newHunksMap = new Map<string, FileHunksData>();
        const changedFiles = new Set<string>();
        for (const result of results) {
          const existing = allFileHunks.get(result.path);
          const newData: FileHunksData = result.error
            ? {
                error: result.error,
                filePath: result.path,
                hunks: [],
                isLoading: false,
                contentHash: result.contentHash,
              }
            : {
                filePath: result.path,
                hunks: result.hunks,
                isLoading: false,
                contentHash: result.contentHash,
              };
          newHunksMap.set(result.path, newData);
          if (!existing || existing.contentHash !== result.contentHash)
            changedFiles.add(result.path);
        }
        if (changedFiles.size > 0) {
          setStaleFilesRef.current(
            (prev) => new Set([...prev, ...changedFiles]),
          );
          setPendingHunksDataRef.current(newHunksMap);
        }
        setLoadingAllHunks(false);
        return;
      }
    } finally {
      if (generation === hunkGenerationRef.current) setLoadingAllHunks(false);
    }
  };

  // File contents can change on disk without any jj-visible status change
  // (e.g. edits to an already-modified file), so the file watcher event
  // above isn't enough to keep an open diff fresh. Re-read everything from
  // disk whenever the window regains focus, so switching back to the app
  // never leaves the Changes tab showing what was there before you left.
  const loadAllFileHunksRef = useRef(loadAllFileHunks);
  loadAllFileHunksRef.current = loadAllFileHunks;
  const filesRef = useRef(files);
  filesRef.current = files;

  const refreshFromDisk = () => {
    void loadChangedFilesRef.current();
    if (filesRef.current.length > 0) {
      loadAllFileHunksRef.current(filesRef.current);
    }
  };

  useEffect(() => {
    const handler = (event: Event) => {
      const { detail } = event as CustomEvent<WorkspaceChangesRefreshDetail>;
      if (
        detail?.workspaceId !== undefined &&
        workspaceId !== undefined &&
        detail.workspaceId !== workspaceId
      ) {
        return;
      }
      refreshFromDisk();
    };
    window.addEventListener(REFRESH_WORKSPACE_CHANGES_EVENT, handler);
    return () => {
      window.removeEventListener(REFRESH_WORKSPACE_CHANGES_EVENT, handler);
    };
  }, [workspaceId]);

  useEffect(() => {
    if (!workspaceId) return;
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    getCurrentWindow()
      .onFocusChanged(({ payload: focused }) => {
        if (!focused) return;
        refreshFromDisk();
      })
      .then((fn) => {
        if (cancelled) fn();
        else unlisten = fn;
      })
      .catch(() => {
        /* not running inside a Tauri window (e.g. tests) */
      });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [workspaceId]);

  useEffect(() => {
    const currentPaths = files.map((f) => f.path);
    const pathsChanged =
      currentPaths.length !== prevFilePathsRef.current.length ||
      currentPaths.some((p, i) => p !== prevFilePathsRef.current[i]);
    if (files.length > 0 && pathsChanged) {
      prevFilePathsRef.current = currentPaths;
      void loadAllFileHunksRef.current(files);
      setLargeChangesetExpandedRef.current(false);
    } else if (files.length === 0 && prevFilePathsRef.current.length > 0) {
      prevFilePathsRef.current = [];
      // Only clear uncommitted hunks. Committed Review-tab hunks are owned by
      // loadChangedFiles and must survive an empty working-copy file list
      // (e.g. right after committing every change while reviewing).
      setAllFileHunks(new Map());
      setLargeChangesetExpandedRef.current(false);
    }
  }, [files, setLargeChangesetExpandedRef]);

  return {
    files,
    setFiles,
    allFileHunks,
    setAllFileHunks,
    loadingAllHunks,
    initialLoading,
    committedFiles,
    committedFileHunks,
    setCommittedFiles,
    liveConflictedFiles,
    invalidateCache,
    refresh,
    loadChangedFiles,
    refreshCommittedChanges,
    loadAllFileHunks,
  };
}
