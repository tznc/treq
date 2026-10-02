import React, { useState } from "react";
import { CheckCircle2, Loader2 } from "lucide-react";
import type { JjFileChange } from "../../lib/api";
import type { ParsedFileChange } from "../../lib/git-utils";
import { isRemoteRepository } from "../../lib/active-repository";
import { useActiveRepository } from "../../lib/active-repository-context";
import { setChangeFilesDragData } from "../../lib/change-file-drag";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "../ui/alert-dialog";
import { ChangesSection } from "../ChangesSection";
import { CommittedChangesSection } from "../CommittedChangesSection";
import { ConflictsSection } from "../ConflictsSection";
import { CommitInput } from "./CommitInput";
import type { CommitAction, CommitInputHandle } from "./types";

interface FileSidebarProps {
  commitInputRef: React.RefObject<CommitInputHandle | null>;
  handleCommit: (msg: string) => void;
  handleCommitAndPush: (msg: string) => void;
  handleCommitAndCreatePR: (msg: string) => void;
  pendingAction: CommitAction | null;
  canCreatePr: boolean;
  hasPr: boolean;
  readOnly: boolean;
  files: ParsedFileChange[];
  commitPending: boolean;
  stagedFiles: Set<string>;
  initialLoading: boolean;
  actualConflictedFiles: string[];
  collapsedSections: Set<string>;
  toggleSectionCollapse: (section: string) => void;
  activeFilePath: string | null;
  setActiveFilePath: React.Dispatch<React.SetStateAction<string | null>>;
  showCommittedChanges: boolean | undefined;
  onToggleShowCommitted: (() => void) | undefined;
  committedFiles: JjFileChange[];
  committedSectionCollapsed: boolean;
  setCommittedSectionCollapsed: React.Dispatch<React.SetStateAction<boolean>>;
  scrollToFileIfNeeded: (path: string) => void;
  stagedFilesList: ParsedFileChange[];
  selectedStagedFiles: Set<string>;
  lastSelectedStagedIndex: number | null;
  handleStagedFileSelect: (path: string, event: React.MouseEvent) => void;
  handleSelectAllStaged: () => void;
  handleUnstageFile: (path: string) => void;
  handleUnstageAllFiles: () => void;
  setSelectedStagedFiles: React.Dispatch<React.SetStateAction<Set<string>>>;
  unstagedFiles: ParsedFileChange[];
  selectedUnstagedFiles: Set<string>;
  lastSelectedFileIndex: number | null;
  handleFileSelect: (path: string, event: React.MouseEvent) => void;
  onMoveFilesToNewWorkspace: ((files: string[]) => void) | undefined;
  handleDiscardAll: () => void;
  handleStashAll: () => void;
  handleDiscardFiles: (path: string) => void;
  setSelectedUnstagedFiles: React.Dispatch<React.SetStateAction<Set<string>>>;
  handleSelectAllUnstaged: () => void;
  handleStageFile: (path: string) => void;
  handleStageAllFiles: () => void;
  fileActionTarget: string | null;
  workspacePath: string;
  sourceBranch: string;
}

export function FileSidebar({
  commitInputRef,
  handleCommit,
  handleCommitAndPush,
  handleCommitAndCreatePR,
  pendingAction,
  canCreatePr,
  hasPr,
  readOnly,
  files,
  commitPending,
  stagedFiles,
  initialLoading,
  actualConflictedFiles,
  collapsedSections,
  toggleSectionCollapse,
  activeFilePath,
  setActiveFilePath,
  showCommittedChanges,
  onToggleShowCommitted,
  committedFiles,
  committedSectionCollapsed,
  setCommittedSectionCollapsed,
  scrollToFileIfNeeded,
  stagedFilesList,
  selectedStagedFiles,
  lastSelectedStagedIndex,
  handleStagedFileSelect,
  handleSelectAllStaged,
  handleUnstageFile,
  handleUnstageAllFiles,
  setSelectedStagedFiles,
  unstagedFiles,
  selectedUnstagedFiles,
  lastSelectedFileIndex,
  handleFileSelect,
  onMoveFilesToNewWorkspace,
  handleDiscardAll,
  handleStashAll,
  handleDiscardFiles,
  setSelectedUnstagedFiles,
  handleSelectAllUnstaged,
  handleStageFile,
  handleStageAllFiles,
  fileActionTarget,
  workspacePath,
  sourceBranch,
}: FileSidebarProps) {
  const [showDiscardAllDialog, setShowDiscardAllDialog] = useState(false);
  // Hidden for remote repositories until the stash modal (list/apply/delete) is routed.
  const isRemote = isRemoteRepository(useActiveRepository());

  const handleFileDragStart = (path: string, event: React.DragEvent) => {
    const files =
      selectedUnstagedFiles.has(path) && selectedUnstagedFiles.size > 0
        ? Array.from(selectedUnstagedFiles)
        : [path];
    setChangeFilesDragData(event.dataTransfer, {
      files,
      sourceBranch,
    });
  };

  return (
    <div className="w-60 border-r border-border bg-sidebar flex flex-col">
      <CommitInput
        ref={commitInputRef}
        onCommit={handleCommit}
        onCommitAndPush={handleCommitAndPush}
        onCommitAndCreatePR={handleCommitAndCreatePR}
        disabled={readOnly || files.length === 0}
        pending={commitPending}
        pendingAction={pendingAction}
        canCreatePr={canCreatePr}
        hasPr={hasPr}
        selectedFileCount={stagedFiles.size}
        totalFileCount={files.length}
        workspacePath={workspacePath}
      />
      <div className="flex-1 overflow-y-auto px-4 pb-4 [&::-webkit-scrollbar]:w-1.5 [&::-webkit-scrollbar-track]:bg-transparent [&::-webkit-scrollbar-thumb]:bg-border [&::-webkit-scrollbar-thumb]:rounded-full">
        {initialLoading ? (
          <div className="flex items-center justify-center h-full">
            <Loader2 className="w-6 h-6 animate-spin text-muted-foreground" />
          </div>
        ) : (
          <>
            {actualConflictedFiles.length > 0 && (
              <ConflictsSection
                files={actualConflictedFiles}
                isCollapsed={collapsedSections.has("conflicts")}
                onToggleCollapse={() => toggleSectionCollapse("conflicts")}
                activeFilePath={activeFilePath}
                onFileSelect={(path) => {
                  setActiveFilePath(path);
                  scrollToFileIfNeeded(path);
                }}
              />
            )}
            {files.length === 0 && committedFiles.length === 0 ? (
              <div className="flex flex-col items-center justify-center h-full text-center py-12">
                <CheckCircle2 className="w-12 h-12 text-muted-foreground/40 mb-3" />
                <p className="text-sm text-muted-foreground">No changes</p>
              </div>
            ) : files.length > 0 ? (
              <>
                {stagedFilesList.length > 0 && (
                  <ChangesSection
                    title="Selected"
                    files={stagedFilesList}
                    isCollapsed={collapsedSections.has("staged")}
                    onToggleCollapse={() => toggleSectionCollapse("staged")}
                    activeFilePath={activeFilePath}
                    selectedFiles={selectedStagedFiles}
                    lastSelectedPath={
                      lastSelectedStagedIndex !== null &&
                      stagedFilesList[lastSelectedStagedIndex]
                        ? stagedFilesList[lastSelectedStagedIndex].path
                        : null
                    }
                    onFileSelect={handleStagedFileSelect}
                    onSelectAll={handleSelectAllStaged}
                    onUnstage={handleUnstageFile}
                    onUnstageAll={handleUnstageAllFiles}
                    onDeselectAll={() => setSelectedStagedFiles(new Set())}
                    isStaged={true}
                    workspacePath={workspacePath}
                  />
                )}
                <ChangesSection
                  title="Changes"
                  files={unstagedFiles}
                  isCollapsed={collapsedSections.has("changes")}
                  onToggleCollapse={() => toggleSectionCollapse("changes")}
                  fileActionTarget={fileActionTarget}
                  activeFilePath={activeFilePath}
                  selectedFiles={selectedUnstagedFiles}
                  lastSelectedPath={
                    lastSelectedFileIndex !== null &&
                    files[lastSelectedFileIndex]
                      ? files[lastSelectedFileIndex].path
                      : null
                  }
                  onFileSelect={handleFileSelect}
                  onMoveToWorkspace={() => {
                    onMoveFilesToNewWorkspace?.(
                      Array.from(selectedUnstagedFiles),
                    );
                  }}
                  onDiscardAll={() => setShowDiscardAllDialog(true)}
                  onStashAll={isRemote ? undefined : handleStashAll}
                  onDiscard={handleDiscardFiles}
                  onDeselectAll={() => setSelectedUnstagedFiles(new Set())}
                  onSelectAll={handleSelectAllUnstaged}
                  onStage={handleStageFile}
                  onStageAll={handleStageAllFiles}
                  workspacePath={workspacePath}
                  onFileDragStart={handleFileDragStart}
                />
              </>
            ) : null}
            {committedFiles.length > 0 && (
              <CommittedChangesSection
                files={committedFiles}
                isCollapsed={committedSectionCollapsed}
                onToggleCollapse={() =>
                  setCommittedSectionCollapsed(!committedSectionCollapsed)
                }
                activeFilePath={activeFilePath}
                onFileSelect={(path, event) => {
                  event.preventDefault();
                  setActiveFilePath(path);
                  scrollToFileIfNeeded(path);
                }}
                workspacePath={workspacePath}
                showCommittedChanges={showCommittedChanges ?? false}
                onToggleShowCommitted={onToggleShowCommitted}
                alwaysVisiblePaths={
                  new Set([
                    ...files.map((file) => file.path),
                    ...actualConflictedFiles,
                  ])
                }
              />
            )}
          </>
        )}
      </div>
      <AlertDialog
        open={showDiscardAllDialog}
        onOpenChange={setShowDiscardAllDialog}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Discard all changes?</AlertDialogTitle>
            <AlertDialogDescription>
              This will discard all {unstagedFiles.length} unsaved{" "}
              {unstagedFiles.length === 1 ? "change" : "changes"}. You can
              restore them from the Undo button on the toast that follows.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction
              onClick={() => {
                setShowDiscardAllDialog(false);
                handleDiscardAll();
              }}
              className="bg-destructive text-destructive-foreground hover:bg-destructive/85 active:bg-destructive/75"
            >
              Discard all changes
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}
