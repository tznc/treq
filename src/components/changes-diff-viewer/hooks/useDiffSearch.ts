import { useEffect, useState } from "react";
import type { ConflictRegion, JjFileChange } from "../../../lib/api";
import { isBinaryFile, type ParsedFileChange } from "../../../lib/git-utils";
import { escapeRegex } from "../../../lib/text-search";
import { useDebounce } from "../../../hooks/useDebounce";
import { useKeyboardShortcut } from "../../../hooks/useKeyboard";
import { filterVisibleCommittedFiles } from "../utils";
import { computeHunkLineNumbers } from "../../diff/hunk-lines";
import type { DiffSearchData, FileHunksData } from "../types";

interface UseDiffSearchParams {
  files: ParsedFileChange[];
  allFileHunks: Map<string, FileHunksData>;
  committedFileHunks: Map<string, FileHunksData>;
  collapsedFiles: Set<string>;
  expandedLargeDiffs: Set<string>;
  showCommittedChanges: boolean;
  committedFiles: JjFileChange[];
  conflictRegionsByFile: Map<string, ConflictRegion[]>;
  conflictLineLookups: Map<string, Map<number, ConflictRegion>>;
  workspacePath: string;
  diffContainerRef: React.RefObject<HTMLDivElement | null>;
  diffScrollApiRef?: React.RefObject<{
    scrollToFile: (path: string) => void;
    scrollToSearchId: (id: string) => void;
  } | null>;
}

export function useDiffSearch({
  files,
  allFileHunks,
  committedFileHunks,
  collapsedFiles,
  expandedLargeDiffs,
  showCommittedChanges,
  committedFiles,
  conflictRegionsByFile,
  conflictLineLookups,
  workspacePath,
  diffContainerRef,
  diffScrollApiRef,
}: UseDiffSearchParams) {
  const [isSearchOpen, setIsSearchOpen] = useState(false);
  const [searchQuery, setSearchQuery] = useState("");
  const debouncedSearchQuery = useDebounce(searchQuery, 150);
  const [currentMatchIndex, setCurrentMatchIndex] = useState(0);
  const [searchFocusTrigger, setSearchFocusTrigger] = useState(0);

  const searchData: DiffSearchData = (() => {
    const matches: DiffSearchData["matches"] = [];
    const matchesByKey = new Map<
      string,
      { firstGlobalIndex: number; count: number }
    >();
    if (!debouncedSearchQuery) return { matches, matchesByKey };
    const escapedQuery = escapeRegex(debouncedSearchQuery);
    const regex = new RegExp(escapedQuery, "gi");
    const countLineMatches = (text: string): number => {
      regex.lastIndex = 0;
      let count = 0;
      while (regex.exec(text) !== null) count++;
      return count;
    };
    const processConflictRegion = (region: ConflictRegion) => {
      const lines =
        region.lines?.length > 0
          ? region.lines.map((line) => line.raw)
          : region.content.split("\n");
      for (let idx = 0; idx < lines.length; idx++) {
        const matchCount = countLineMatches(lines[idx]);
        if (matchCount > 0) {
          const key = `conflict:${region.id}:${idx}`;
          matchesByKey.set(key, {
            count: matchCount,
            firstGlobalIndex: matches.length,
          });
          for (let matchIdx = 0; matchIdx < matchCount; matchIdx++) {
            matches.push({
              filePath: region.file_path,
              hunkIndex: -1,
              lineIndex: idx,
              matchIndexInLine: matchIdx,
            });
          }
        }
      }
    };
    for (const [, regions] of conflictRegionsByFile) {
      for (const region of regions) processConflictRegion(region);
    }
    const processFile = (
      filePath: string,
      fileHunksMap: Map<string, FileHunksData>,
    ) => {
      const fileData = fileHunksMap.get(filePath);
      if (!fileData || fileData.isLoading || !fileData.hunks) return;
      if (collapsedFiles.has(filePath)) return;
      if (isBinaryFile(filePath)) return;
      const conflictLineMap = conflictLineLookups.get(filePath);
      let additions = 0,
        deletions = 0;
      for (const hunk of fileData.hunks) {
        for (const line of hunk.lines) {
          if (line.startsWith("+")) additions++;
          else if (line.startsWith("-")) deletions++;
        }
      }
      if (additions + deletions > 250 && !expandedLargeDiffs.has(filePath))
        return;
      for (let hunkIndex = 0; hunkIndex < fileData.hunks.length; hunkIndex++) {
        const hunk = fileData.hunks[hunkIndex];
        const lineNumbers = computeHunkLineNumbers(hunk);
        for (let lineIndex = 0; lineIndex < hunk.lines.length; lineIndex++) {
          const newLineNumber = lineNumbers[lineIndex]?.new;
          if (
            newLineNumber !== undefined &&
            conflictLineMap?.has(newLineNumber)
          )
            continue;
          const lineText = hunk.lines[lineIndex].substring(1);
          const mc = countLineMatches(lineText);
          if (mc > 0) {
            const key = `${filePath}:${hunkIndex}:${lineIndex}`;
            matchesByKey.set(key, {
              count: mc,
              firstGlobalIndex: matches.length,
            });
            for (let matchIdx = 0; matchIdx < mc; matchIdx++) {
              matches.push({
                filePath,
                hunkIndex,
                lineIndex,
                matchIndexInLine: matchIdx,
              });
            }
          }
        }
      }
    };
    for (const file of files) processFile(file.path, allFileHunks);
    const alwaysVisible = new Set([
      ...files.map((file) => file.path),
      ...conflictLineLookups.keys(),
    ]);
    for (const file of filterVisibleCommittedFiles(
      committedFiles,
      showCommittedChanges,
      alwaysVisible,
    )) {
      processFile(file.path, committedFileHunks);
    }
    return { matches, matchesByKey };
  })();

  useEffect(() => {
    if (searchData.matches.length === 0) {
      setCurrentMatchIndex(0);
    } else if (currentMatchIndex >= searchData.matches.length) {
      setCurrentMatchIndex(0);
    }
  }, [searchData.matches.length]);

  useEffect(() => {
    setIsSearchOpen(false);
    setSearchQuery("");
    setCurrentMatchIndex(0);
  }, [workspacePath]);

  useKeyboardShortcut(
    "f",
    true,
    () => {
      setIsSearchOpen(true);
      setSearchFocusTrigger((prev) => prev + 1);
    },
    [],
  );

  const scrollToSearchMatch = (matchIdx: number) => {
    if (!diffContainerRef.current || searchData.matches.length === 0) return;
    const match = searchData.matches[matchIdx];
    if (!match) return;
    const searchId =
      match.hunkIndex === -1
        ? `conflict:${match.filePath}:${match.lineIndex}`
        : `${match.filePath}:${match.hunkIndex}:${match.lineIndex}`;
    const api = diffScrollApiRef?.current;
    if (api) {
      api.scrollToSearchId(searchId);
      return;
    }
    const el = diffContainerRef.current.querySelector(
      `[data-search-id="${CSS.escape(searchId)}"]`,
    );
    if (el) el.scrollIntoView({ behavior: "smooth", block: "center" });
  };

  const handleSearchNext = () => {
    if (searchData.matches.length === 0) return;
    const next = (currentMatchIndex + 1) % searchData.matches.length;
    setCurrentMatchIndex(next);
    scrollToSearchMatch(next);
  };

  const handleSearchPrevious = () => {
    if (searchData.matches.length === 0) return;
    const prev =
      (currentMatchIndex - 1 + searchData.matches.length) %
      searchData.matches.length;
    setCurrentMatchIndex(prev);
    scrollToSearchMatch(prev);
  };

  const handleSearchClose = () => {
    setIsSearchOpen(false);
    setSearchQuery("");
    setCurrentMatchIndex(0);
  };

  return {
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
  };
}
