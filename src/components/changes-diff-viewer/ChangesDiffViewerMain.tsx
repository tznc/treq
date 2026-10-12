/* eslint-disable max-lines */

import React, { useRef, useState } from "react";
import { type ParsedFileChange } from "../../lib/git-utils";
import { useDiffSettingsStore } from "../../stores/diffSettingsStore";
import { useDiffSearch } from "./hooks/useDiffSearch";
import { useLineSelection } from "./hooks/useLineSelection";
import { useComments } from "./hooks/useComments";
import { useGithubReviewThreads } from "./hooks/useGithubReviewThreads";
import { useFileStaging } from "./hooks/useFileStaging";
import { useFileLoading } from "./hooks/useFileLoading";
import { useReview } from "./hooks/useReview";
import { useAgentReviewComments } from "./hooks/useAgentReviewComments";
import { AgentReviewContext } from "./AgentReviewContext";
import { useConflicts } from "./hooks/useConflicts.ts";
import { useFileActions } from "./hooks/useFileActions";
import { useToast } from "../ui/toast";
import { ContextMenu } from "./ContextMenu";
import { ReviewActionBar } from "./ReviewActionBar";
import { DiffContentArea } from "./DiffContentArea";
import { FileSidebar } from "./FileSidebar";
import { filesEqual } from "./utils";
import { HOME_MOVE_ENDPOINT } from "../../lib/change-file-drag";
import { stashWorkspaceChanges } from "../../lib/api";
import { AGENT_REVIEW_TARGET_WORKSPACE_DIFF } from "../../lib/api-types-review";
import { prepareWorkspaceReview } from "../../lib/agent-review-launch";
import { invalidateReviewChangeCount } from "../../lib/review-change-count";
import { invalidateQueries } from "../../lib/swr-cache";
import type {
  ChangesDiffViewerProps,
  CommitInputHandle,
  FileHunksData,
} from "./types";

export const ChangesDiffViewer = ({
  workspacePath,
  repoPath,
  workspaceId,
  isHomeRepo = false,
  branchName,
  readOnly = false,
  onStagedFilesChange,
  onChangedFilesChange,
  onRefreshingChange,
  initialSelectedFile,
  onReviewSubmitted,
  onCreateAgentWithReview,
  onStartAgentReview,
  onSendAgentReviewCommentToAgent,
  conflictedFiles = [],
  showCommittedChanges = false,
  onShowCommittedChangesChange,
  onMoveFilesToNewWorkspace,
  workspace,
  baseBranch,
}: ChangesDiffViewerProps) => {
  const { addToast } = useToast();
  const { fontSize: diffFontSize } = useDiffSettingsStore();

  const [activeFilePath, setActiveFilePath] = useState<string | null>(null);
  const [committedSectionCollapsed, setCommittedSectionCollapsed] =
    useState(false);
  const conflictFileRefs = useRef<Map<string, HTMLDivElement>>(new Map());
  const commitInputRef = useRef<CommitInputHandle>(null);
  const diffContainerRef = useRef<HTMLDivElement>(null);
  const diffScrollApiRef = useRef<{
    scrollToFile: (path: string) => void;
    scrollToSearchId: (id: string) => void;
  } | null>(null);
  const isReloadingRef = useRef<boolean>(false);
  const applyChangedFilesRef = useRef<
    (parsed: ParsedFileChange[], forceApply?: boolean) => void
  >(() => {});
  const isInReviewModeRef = useRef<boolean>(false);
  const setStaleFilesRef = useRef<
    React.Dispatch<React.SetStateAction<Set<string>>>
  >(() => {});
  const setPendingHunksDataRef = useRef<
    React.Dispatch<React.SetStateAction<Map<string, FileHunksData> | null>>
  >(() => {});
  const setLargeChangesetExpandedRef = useRef<
    React.Dispatch<React.SetStateAction<boolean>>
  >(() => {});

  const {
    files,
    setFiles,
    allFileHunks,
    setAllFileHunks,
    committedFileHunks,
    loadingAllHunks,
    initialLoading,
    committedFiles,
    liveConflictedFiles,
    invalidateCache,
    loadChangedFiles,
    refreshCommittedChanges,
  } = useFileLoading({
    workspacePath,
    repoPath,
    workspaceId,
    isHomeRepo,
    showCommittedChanges,
    conflictedFilesHint: conflictedFiles,
    onRefreshingChange,
    setLargeChangesetExpandedRef,
    applyChangedFilesRef,
    isInReviewModeRef,
    setStaleFilesRef,
    setPendingHunksDataRef,
    isReloadingRef,
    addToast,
  });
  // Prefer the latest workspace_diff conflict list over the status prop —
  // status can lag behind a WC marker resolve until files-changed refetches it.
  const conflictedFilesForUi = liveConflictedFiles ?? conflictedFiles;
  const {
    actualConflictedFiles,
    conflictRegionsByFile,
    conflictLineLookups,
    firstConflictRegionIdByFile,
  } = useConflicts({
    allFileHunks,
    committedFileHunks,
    conflictedFilesHint: conflictedFilesForUi,
  });

  const {
    stagedFiles,
    setStagedFiles,
    selectedUnstagedFiles,
    setSelectedUnstagedFiles,
    lastSelectedFileIndex,
    selectedStagedFiles,
    setSelectedStagedFiles,
    lastSelectedStagedIndex,
    collapsedFiles,
    setCollapsedFiles,
    collapsedSections,
    expandedLargeDiffs,
    largeChangesetExpanded,
    setLargeChangesetExpanded,
    unstagedFiles,
    stagedFilesList,
    scrollToFileIfNeeded,
    handleFileSelect,
    handleStageFile,
    handleUnstageFile,
    handleUnstageAllFiles,
    handleStagedFileSelect,
    toggleFileCollapse,
    toggleLargeDiff,
    toggleSectionCollapse,
    handleStageAllFiles,
    handleSelectAllUnstaged,
    handleSelectAllStaged,
  } = useFileStaging({ files, diffContainerRef, diffScrollApiRef });
  setLargeChangesetExpandedRef.current = setLargeChangesetExpanded;

  const {
    isSearchOpen,
    searchQuery,
    setSearchQuery,
    searchData,
    debouncedSearchQuery,
    currentMatchIndex,
    setCurrentMatchIndex,
    searchFocusTrigger,
    handleSearchNext,
    handleSearchPrevious,
    handleSearchClose,
  } = useDiffSearch({
    files,
    allFileHunks,
    collapsedFiles,
    expandedLargeDiffs,
    showCommittedChanges: showCommittedChanges ?? false,
    committedFiles,
    committedFileHunks,
    conflictRegionsByFile,
    conflictLineLookups,
    workspacePath,
    diffContainerRef,
    diffScrollApiRef,
  });

  const {
    diffLineSelection,
    contextMenuPosition,
    setContextMenuPosition,
    clearSelection,
    handleLineMouseDown,
    handleLineMouseEnter,
    handleLineMouseUp,
    handleBackgroundClick,
    handleContextMenu,
    isLineSelected,
  } = useLineSelection({
    allFileHunks,
    committedFileHunks,
    onClearFileSelections: () => {
      setSelectedUnstagedFiles(new Set());
      setSelectedStagedFiles(new Set());
    },
  });

  const {
    comments,
    setComments,
    conflictComments,
    setConflictComments,
    openConflictComments,
    setOpenConflictComments,
    editingConflictCommentId,
    showCommentInput,
    setShowCommentInput,
    pendingComment,
    setPendingComment,
    editingCommentId,
    hasUserAddedComments,
    setHasUserAddedComments,
    addComment,
    handleAddCommentFromSelection,
    cancelComment,
    deleteComment,
    startEditComment,
    cancelEditComment,
    saveEditComment,
    saveConflictComment,
    clearConflictComment,
    startEditConflictComment,
    cancelEditConflictComment,
    saveEditConflictComment,
    toggleConflictComment,
    getOutdatedCommentsForFile,
    getFileCommentsForFile,
    getAllOutdatedComments,
    handleCopyOutdatedComments,
    getCommentsForLine,
  } = useComments({
    allFileHunks,
    committedFileHunks,
    diffLineSelection,
    clearSelection,
    setContextMenuPosition,
    addToast,
  });

  const {
    getThreadsForLine,
    getUnplacedThreadsForFile,
    collapsedThreadIds,
    toggleThreadCollapse,
  } = useGithubReviewThreads({
    repoPath,
    branchName,
    allFileHunks,
    committedFileHunks,
  });

  const {
    reviewPopoverOpen,
    setReviewPopoverOpen,
    finalReviewComment,
    setFinalReviewComment,
    showCancelDialog,
    setShowCancelDialog,
    copiedReview,
    sendingReview,
    staleFiles,
    setStaleFiles,
    setPendingFilesData,
    setPendingHunksData,
    viewedFiles,
    handleRequestChanges,
    handleCancelReview,
    handleCopyReview,
    handleReloadWithPendingChanges,
    handleMarkFileViewed,
    handleUnmarkFileViewed,
  } = useReview({
    comments,
    setComments,
    conflictComments,
    setConflictComments,
    setHasUserAddedComments,
    allFileHunks,
    setAllFileHunks,
    files,
    workspacePath,
    repoPath,
    workspaceId,
    conflictRegionsByFile,
    actualConflictedFiles,
    setCollapsedFiles,
    applyChangedFilesRef,
    isReloadingRef,
    onCreateAgentWithReview,
    onReviewSubmitted,
    addToast,
  });

  const isInReviewMode =
    hasUserAddedComments ||
    showCommentInput ||
    reviewPopoverOpen ||
    finalReviewComment.trim().length > 0;

  isInReviewModeRef.current = isInReviewMode;
  setStaleFilesRef.current = setStaleFiles;
  setPendingHunksDataRef.current = setPendingHunksData;

  const applyChangedFiles = (
    parsed: ParsedFileChange[],
    forceApply = false,
  ) => {
    onChangedFilesChange?.(parsed);
    if (isInReviewMode && !forceApply) {
      setFiles((prev) => {
        if (prev.length === 0) {
          return filesEqual(prev, parsed) ? prev : parsed;
        }
        const prevPaths = new Set(prev.map((f) => f.path));
        const newPaths = new Set(parsed.map((f) => f.path));
        const changedFiles = new Set<string>();
        for (const path of newPaths) {
          if (!prevPaths.has(path)) changedFiles.add(path);
        }
        for (const path of prevPaths) {
          if (!newPaths.has(path)) changedFiles.add(path);
        }
        for (const newFile of parsed) {
          const oldFile = prev.find((f) => f.path === newFile.path);
          if (
            oldFile &&
            (oldFile.stagedStatus !== newFile.stagedStatus ||
              oldFile.workspaceStatus !== newFile.workspaceStatus)
          ) {
            changedFiles.add(newFile.path);
          }
        }
        if (changedFiles.size > 0) {
          setStaleFiles(
            (prevStale) => new Set([...prevStale, ...changedFiles]),
          );
          setPendingFilesData(parsed);
        }
        return prev;
      });
      return;
    }
    setFiles((prev) => (filesEqual(prev, parsed) ? prev : parsed));
    setStaleFiles(new Set());
    setPendingFilesData(null);
    setPendingHunksData(null);
    if (
      initialSelectedFile &&
      parsed.some((f) => f.path === initialSelectedFile)
    ) {
      setSelectedUnstagedFiles(new Set([initialSelectedFile]));
    }
    if (onStagedFilesChange) {
      const staged = parsed
        .filter((f) => f.stagedStatus && f.stagedStatus !== " ")
        .map((f) => f.path);
      onStagedFilesChange(Array.from(new Set(staged)));
    }
  };
  applyChangedFilesRef.current = applyChangedFiles;

  const {
    fileActionTarget,
    expandedContext,
    commitPending,
    pendingAction,
    canCreatePr,
    hasPr,
    handleDiscardAll,
    handleDiscardFiles,
    handleCopyLineLocation,
    handleCopyLines,
    handleCommit,
    handleCommitAndPush,
    handleCommitAndCreatePR,
    handleExpandContext,
  } = useFileActions({
    workspacePath,
    repoPath,
    workspaceId,
    workspace,
    baseBranch,
    readOnly,
    selectedUnstagedFiles,
    stagedFiles,
    setStagedFiles,
    setSelectedUnstagedFiles,
    allFileHunks,
    diffLineSelection,
    setContextMenuPosition,
    invalidateCache,
    loadChangedFiles,
    refreshCommittedChanges,
    setCommittedSectionCollapsed,
    addToast,
  });

  const handleStashAll = async () => {
    if (readOnly || !repoPath) return;
    try {
      const entry = await stashWorkspaceChanges(repoPath, workspaceId ?? null);
      addToast({
        description: `Stashed ${entry.files_changed.length} file${entry.files_changed.length === 1 ? "" : "s"} (${entry.short_commit_id})`,
        title: "Changes stashed",
        type: "success",
      });
      await invalidateCache();
      await loadChangedFiles();
      await invalidateReviewChangeCount(repoPath, workspaceId);
      await invalidateQueries(["stashes", repoPath]);
    } catch (error) {
      addToast({
        description: error instanceof Error ? error.message : String(error),
        title: "Stash Failed",
        type: "error",
      });
    }
  };

  // Local agent review comments target this workspace's diff. `targetType` is
  // a discriminator, not a constant, so other reviewable content types can
  // reuse the same storage and UI later.
  const agentReviewTargetId =
    workspaceId != null ? String(workspaceId) : undefined;
  const {
    openAgentReviewComments,
    getAgentCommentsForLine,
    resolveAgentComment,
    deleteAgentComment,
    applyAgentSuggestion,
  } = useAgentReviewComments({
    repoPath,
    targetType: AGENT_REVIEW_TARGET_WORKSPACE_DIFF,
    targetId: agentReviewTargetId,
  });

  const [startingAgentReview, setStartingAgentReview] = useState(false);

  const handleStartAgentReview = async () => {
    if (!onStartAgentReview || !agentReviewTargetId) return;
    setStartingAgentReview(true);
    try {
      const { prompt, agent } = await prepareWorkspaceReview({
        repoPath,
        workspaceId: Number(agentReviewTargetId),
        branchName,
        fileCount: files.length,
      });
      await onStartAgentReview(prompt, agent);
    } catch (error) {
      addToast({
        description: error instanceof Error ? error.message : String(error),
        title: "Failed to start review",
        type: "error",
      });
    } finally {
      setStartingAgentReview(false);
    }
  };

  const handleAgentCommentError = (message: string) => {
    addToast({
      description: message,
      title: "Review comment action failed",
      type: "error",
    });
  };

  const applyAgentSuggestionAndReload = async (commentId: string) => {
    await applyAgentSuggestion(commentId);
    await invalidateCache();
    await loadChangedFiles();
  };

  const hasConflicts = actualConflictedFiles.length > 0;
  const totalComments = comments.length + conflictComments.size;
  const showActionBar = hasConflicts || comments.length > 0;

  return (
    <AgentReviewContext.Provider
      value={{
        getAgentCommentsForLine,
        resolveAgentComment,
        deleteAgentComment,
        applyAgentSuggestion: applyAgentSuggestionAndReload,
        sendAgentCommentToAgent: onSendAgentReviewCommentToAgent,
        onAgentCommentError: handleAgentCommentError,
      }}
    >
      <div
        className="flex h-full overflow-hidden"
        data-testid="changes-diff-viewer"
        onClick={handleBackgroundClick}
      >
        <FileSidebar
          commitInputRef={commitInputRef}
          handleCommit={handleCommit}
          handleCommitAndPush={handleCommitAndPush}
          handleCommitAndCreatePR={handleCommitAndCreatePR}
          pendingAction={pendingAction}
          canCreatePr={canCreatePr}
          hasPr={hasPr}
          readOnly={readOnly}
          files={files}
          commitPending={commitPending}
          stagedFiles={stagedFiles}
          initialLoading={initialLoading}
          actualConflictedFiles={actualConflictedFiles}
          collapsedSections={collapsedSections}
          toggleSectionCollapse={toggleSectionCollapse}
          activeFilePath={activeFilePath}
          setActiveFilePath={setActiveFilePath}
          showCommittedChanges={showCommittedChanges}
          onToggleShowCommitted={
            onShowCommittedChangesChange
              ? () => onShowCommittedChangesChange(!showCommittedChanges)
              : undefined
          }
          committedFiles={committedFiles}
          committedSectionCollapsed={committedSectionCollapsed}
          setCommittedSectionCollapsed={setCommittedSectionCollapsed}
          scrollToFileIfNeeded={scrollToFileIfNeeded}
          stagedFilesList={stagedFilesList}
          selectedStagedFiles={selectedStagedFiles}
          lastSelectedStagedIndex={lastSelectedStagedIndex}
          handleStagedFileSelect={handleStagedFileSelect}
          handleSelectAllStaged={handleSelectAllStaged}
          handleUnstageFile={handleUnstageFile}
          handleUnstageAllFiles={handleUnstageAllFiles}
          setSelectedStagedFiles={setSelectedStagedFiles}
          unstagedFiles={unstagedFiles}
          selectedUnstagedFiles={selectedUnstagedFiles}
          lastSelectedFileIndex={lastSelectedFileIndex}
          handleFileSelect={handleFileSelect}
          onMoveFilesToNewWorkspace={onMoveFilesToNewWorkspace}
          handleDiscardAll={handleDiscardAll}
          handleStashAll={handleStashAll}
          handleDiscardFiles={handleDiscardFiles}
          setSelectedUnstagedFiles={setSelectedUnstagedFiles}
          handleSelectAllUnstaged={handleSelectAllUnstaged}
          handleStageFile={handleStageFile}
          handleStageAllFiles={handleStageAllFiles}
          fileActionTarget={fileActionTarget}
          workspacePath={workspacePath}
          sourceBranch={branchName ?? HOME_MOVE_ENDPOINT}
        />

        <div className="flex-1 flex flex-col min-w-0">
          <ReviewActionBar
            showActionBar={showActionBar}
            hasConflicts={hasConflicts}
            totalComments={totalComments}
            comments={comments}
            staleFiles={staleFiles}
            reviewPopoverOpen={reviewPopoverOpen}
            setReviewPopoverOpen={setReviewPopoverOpen}
            finalReviewComment={finalReviewComment}
            setFinalReviewComment={setFinalReviewComment}
            showCancelDialog={showCancelDialog}
            setShowCancelDialog={setShowCancelDialog}
            copiedReview={copiedReview}
            sendingReview={sendingReview}
            handleCopyReview={handleCopyReview}
            handleCancelReview={handleCancelReview}
            handleRequestChanges={handleRequestChanges}
            handleReloadWithPendingChanges={handleReloadWithPendingChanges}
            getAllOutdatedComments={getAllOutdatedComments}
            handleCopyOutdatedComments={handleCopyOutdatedComments}
            handleStartAgentReview={
              onStartAgentReview && agentReviewTargetId
                ? () => void handleStartAgentReview()
                : undefined
            }
            startingAgentReview={startingAgentReview}
            agentReviewCommentCount={openAgentReviewComments.length}
          />
          <DiffContentArea
            isSearchOpen={isSearchOpen}
            searchQuery={searchQuery}
            setSearchQuery={setSearchQuery}
            currentMatchIndex={currentMatchIndex}
            setCurrentMatchIndex={setCurrentMatchIndex}
            handleSearchNext={handleSearchNext}
            handleSearchPrevious={handleSearchPrevious}
            handleSearchClose={handleSearchClose}
            searchFocusTrigger={searchFocusTrigger}
            searchData={searchData}
            debouncedSearchQuery={debouncedSearchQuery}
            initialLoading={initialLoading}
            loadingAllHunks={loadingAllHunks}
            files={files}
            allFileHunks={allFileHunks}
            committedFiles={committedFiles}
            committedFileHunks={committedFileHunks}
            showCommittedChanges={showCommittedChanges}
            largeChangesetExpanded={largeChangesetExpanded}
            setLargeChangesetExpanded={setLargeChangesetExpanded}
            actualConflictedFiles={actualConflictedFiles}
            conflictLineLookups={conflictLineLookups}
            firstConflictRegionIdByFile={firstConflictRegionIdByFile}
            expandedContext={expandedContext}
            conflictComments={conflictComments}
            openConflictComments={openConflictComments}
            editingConflictCommentId={editingConflictCommentId}
            diffLineSelection={diffLineSelection}
            showCommentInput={showCommentInput}
            pendingComment={pendingComment}
            editingCommentId={editingCommentId}
            comments={comments}
            conflictFileRefs={conflictFileRefs}
            diffFontSize={diffFontSize}
            handleExpandContext={handleExpandContext}
            handleLineMouseDown={handleLineMouseDown}
            handleLineMouseEnter={handleLineMouseEnter}
            handleLineMouseUp={handleLineMouseUp}
            handleAddCommentFromSelection={handleAddCommentFromSelection}
            isLineSelected={isLineSelected}
            saveConflictComment={saveConflictComment}
            clearConflictComment={clearConflictComment}
            toggleConflictComment={toggleConflictComment}
            setOpenConflictComments={setOpenConflictComments}
            startEditConflictComment={startEditConflictComment}
            cancelEditConflictComment={cancelEditConflictComment}
            saveEditConflictComment={saveEditConflictComment}
            addComment={addComment}
            cancelComment={cancelComment}
            deleteComment={deleteComment}
            startEditComment={startEditComment}
            cancelEditComment={cancelEditComment}
            saveEditComment={saveEditComment}
            setPendingComment={setPendingComment}
            setShowCommentInput={setShowCommentInput}
            getCommentsForLine={getCommentsForLine}
            getThreadsForLine={getThreadsForLine}
            getUnplacedThreadsForFile={getUnplacedThreadsForFile}
            collapsedThreadIds={collapsedThreadIds}
            toggleThreadCollapse={toggleThreadCollapse}
            collapsedFiles={collapsedFiles}
            viewedFiles={viewedFiles}
            expandedLargeDiffs={expandedLargeDiffs}
            readOnly={readOnly}
            fileActionTarget={fileActionTarget}
            selectedUnstagedFiles={selectedUnstagedFiles}
            workspacePath={workspacePath}
            toggleFileCollapse={toggleFileCollapse}
            toggleLargeDiff={toggleLargeDiff}
            handleMarkFileViewed={handleMarkFileViewed}
            handleUnmarkFileViewed={handleUnmarkFileViewed}
            handleDiscardFiles={handleDiscardFiles}
            handleContextMenu={handleContextMenu}
            addToast={addToast}
            getOutdatedCommentsForFile={getOutdatedCommentsForFile}
            getFileCommentsForFile={getFileCommentsForFile}
            diffContainerRef={diffContainerRef}
            diffScrollApiRef={diffScrollApiRef}
          />
        </div>

        {contextMenuPosition &&
          diffLineSelection &&
          diffLineSelection.lines.length > 0 && (
            <ContextMenu
              position={contextMenuPosition}
              onAddComment={handleAddCommentFromSelection}
              onCopyLocation={handleCopyLineLocation}
              onCopyLines={handleCopyLines}
            />
          )}
      </div>
    </AgentReviewContext.Provider>
  );
};

ChangesDiffViewer.displayName = "ChangesDiffViewer";
