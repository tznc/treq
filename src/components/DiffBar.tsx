import type { WorkspaceDiffStats } from "../lib/workspace-stack";

/** Half-width of each side of the bar (insertions grow left, deletions right). */
const DIFF_BAR_HALF_WIDTH_PX = 14;

/**
 * Gerrit-style relative +/- bar used on the Code tab stack card and the
 * Merge Queue stack cards. Each side scales against `maxChange`.
 */
export function DiffBar({
  diffStats,
  maxChange,
}: {
  diffStats: WorkspaceDiffStats;
  maxChange: number;
}) {
  const { insertions, deletions } = diffStats;
  if (insertions === 0 && deletions === 0) return null;

  const insertionWidth =
    maxChange === 0 || insertions === 0
      ? 0
      : Math.max(
          1,
          Math.round((insertions / maxChange) * DIFF_BAR_HALF_WIDTH_PX),
        );
  const deletionWidth =
    maxChange === 0 || deletions === 0
      ? 0
      : Math.max(
          1,
          Math.round((deletions / maxChange) * DIFF_BAR_HALF_WIDTH_PX),
        );

  return (
    <div
      className="flex h-1.5 flex-shrink-0 items-center"
      data-testid="diff-bar"
      title={`+${insertions} -${deletions}`}
    >
      <div
        className="flex h-full justify-end"
        style={{ width: DIFF_BAR_HALF_WIDTH_PX }}
      >
        <div
          className="h-full bg-green-600 dark:bg-green-400"
          style={{ width: insertionWidth }}
        />
      </div>
      <div className="w-px h-full bg-border" aria-hidden="true" />
      <div
        className="flex h-full justify-start"
        style={{ width: DIFF_BAR_HALF_WIDTH_PX }}
      >
        <div
          className="h-full bg-red-600 dark:bg-red-400"
          style={{ width: deletionWidth }}
        />
      </div>
    </div>
  );
}

/** Inline +N / bar / -N group used next to a stack or queue entry. */
export function DiffStatsInline({
  insertions,
  deletions,
  maxChange,
}: {
  insertions: number;
  deletions: number;
  maxChange: number;
}) {
  if (insertions === 0 && deletions === 0) return null;

  return (
    <div
      className="ml-auto flex items-center gap-1.5"
      data-testid="diff-stats-inline"
    >
      <span className="text-xs font-mono text-green-600 dark:text-green-400">
        +{insertions}
      </span>
      <DiffBar diffStats={{ insertions, deletions }} maxChange={maxChange} />
      <span className="text-xs font-mono text-red-600 dark:text-red-400">
        -{deletions}
      </span>
    </div>
  );
}
