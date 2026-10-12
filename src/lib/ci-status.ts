import { CheckCircle2, Loader2, XCircle } from "lucide-react";

/** Rolled-up CI state, matching `PrCiStatus.state` from `gh pr checks`. */
export type CiState = "success" | "failure" | "pending";

interface CiStateStyle {
  label: string;
  Icon: typeof CheckCircle2;
  /** Border + hover styling for the compact pill button (workspace header, PR page summary). */
  buttonClassName: string;
  /** Plain icon/text color for a per-check row (no border/hover chrome). */
  textClassName: string;
}

/**
 * Single source of truth for CI status colors/icons/labels, shared between
 * the workspace header's CiStatusIndicator and the GitHub panel's PR detail
 * "Checks" list, so both surfaces always look and mean the same thing.
 */
const CI_STATE_STYLES: Record<CiState, CiStateStyle> = {
  success: {
    label: "CI passed",
    Icon: CheckCircle2,
    buttonClassName:
      "border-green-600 text-green-700 hover:bg-green-600/10 hover:text-green-700 dark:text-green-400 dark:border-green-500",
    textClassName: "text-green-700 dark:text-green-400",
  },
  failure: {
    label: "CI failed",
    Icon: XCircle,
    buttonClassName:
      "border-red-600 text-red-700 hover:bg-red-600/10 hover:text-red-700 dark:text-red-400 dark:border-red-500",
    textClassName: "text-red-700 dark:text-red-400",
  },
  pending: {
    label: "CI running",
    Icon: Loader2,
    buttonClassName:
      "border-yellow-600 text-yellow-700 hover:bg-yellow-600/10 hover:text-yellow-700 dark:text-yellow-400 dark:border-yellow-500",
    textClassName: "text-yellow-700 dark:text-yellow-400",
  },
};

export function formatCheckDuration(secs: number): string {
  const total = Math.max(0, Math.floor(secs));
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const seconds = total % 60;
  if (hours > 0) {
    return `${hours}h ${minutes}m ${seconds}s`;
  }
  if (minutes > 0) {
    return `${minutes}m ${seconds}s`;
  }
  return `${seconds}s`;
}

/** Looks up the style for a `PrCiStatus.state` string, defaulting to pending for unknown values. */
export function ciStatusStyle(state: string): CiStateStyle {
  return CI_STATE_STYLES[state as CiState] ?? CI_STATE_STYLES.pending;
}

/** Maps a `gh pr checks` bucket ("pass"|"fail"|"pending"|"skipping"|"cancel") to a display state. */
export function ciStateForBucket(bucket: string): CiState {
  if (bucket === "pass" || bucket === "skipping") return "success";
  if (bucket === "fail" || bucket === "cancel") return "failure";
  return "pending";
}

/** Sort key for check lists: failure, then pending, then success. */
const CI_STATE_SEVERITY: Record<CiState, number> = {
  failure: 0,
  pending: 1,
  success: 2,
};

/** Comparator for check rows — failed first, then pending, then passed/skipped. */
export function compareCiChecksBySeverity(
  a: { bucket: string },
  b: { bucket: string },
): number {
  return (
    CI_STATE_SEVERITY[ciStateForBucket(a.bucket)] -
    CI_STATE_SEVERITY[ciStateForBucket(b.bucket)]
  );
}
