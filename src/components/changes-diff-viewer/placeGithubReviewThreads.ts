import type { GhReviewThread } from "../../lib/api-types";
import { computeHunkLineNumbers } from "../diff/hunk-lines";
import type { FileHunksData } from "./types";

function pushToMap(
  map: Map<string, GhReviewThread[]>,
  key: string,
  thread: GhReviewThread,
) {
  const existing = map.get(key);
  if (existing) existing.push(thread);
  else map.set(key, [thread]);
}

function lineSideOf(ln: { new?: number; old?: number }): "old" | "new" {
  return ln.old !== undefined && ln.new === undefined ? "old" : "new";
}

/**
 * Try to anchor a thread onto a visible diff line in any of the provided
 * hunk maps. Returns the line-key used for inline rendering, or null if the
 * thread does not match any currently visible line.
 *
 * GitHub's `isOutdated` flag is intentionally ignored here: a thread is only
 * treated as unplaced when we cannot find its line in the local diff. The
 * Review tab often shows committed PR hunks while GitHub may still mark the
 * thread outdated after a force-push that left the line intact.
 */
export function tryPlaceThread(
  thread: GhReviewThread,
  hunkMaps: Array<Map<string, FileHunksData>>,
): string | null {
  if (thread.line == null) return null;

  const expectedSide: "old" | "new" =
    thread.diff_side === "LEFT" ? "old" : "new";

  for (const hunksMap of hunkMaps) {
    const fileData = hunksMap.get(thread.path);
    if (!fileData?.hunks) continue;

    for (const hunk of fileData.hunks) {
      const lineNumbers = computeHunkLineNumbers(hunk);
      const hasMatch = lineNumbers.some((ln) => {
        const actualLineNum = ln.new ?? ln.old ?? 0;
        return actualLineNum === thread.line && lineSideOf(ln) === expectedSide;
      });
      if (hasMatch) {
        return `${thread.path}:${hunk.id}:${thread.line}:${expectedSide}`;
      }
    }
  }

  return null;
}

export function placeGithubReviewThreads(
  threads: GhReviewThread[],
  hunkMaps: Array<Map<string, FileHunksData>>,
): {
  threadsByLineKey: Map<string, GhReviewThread[]>;
  unplacedThreadsByFile: Map<string, GhReviewThread[]>;
} {
  const byLineKey = new Map<string, GhReviewThread[]>();
  const unplaced = new Map<string, GhReviewThread[]>();

  for (const thread of threads) {
    const key = tryPlaceThread(thread, hunkMaps);
    if (key) pushToMap(byLineKey, key, thread);
    else pushToMap(unplaced, thread.path, thread);
  }

  return { threadsByLineKey: byLineKey, unplacedThreadsByFile: unplaced };
}
