/**
 * Status colors for different change types (works with jj and git)
 */
const STATUS_COLORS = {
  "??": {
    bg: "bg-green-500",
    text: "text-green-500",
  },
  A: {
    bg: "bg-green-500",
    text: "text-green-500",
  },
  D: {
    bg: "bg-red-500",
    text: "text-red-500",
  },
  M: {
    bg: "bg-yellow-500",
    text: "text-yellow-500",
  },
  R: {
    bg: "bg-blue-500",
    text: "text-blue-500",
  },
} as const;

/**
 * Get background color class for a status code
 */
export function getStatusBgColor(status?: string): string {
  if (!status) return "bg-muted";
  return STATUS_COLORS[status as keyof typeof STATUS_COLORS]?.bg || "bg-muted";
}

/**
 * Get text color class for a file status
 * Simplified to work with any status code
 */
export function getFileStatusTextColor(
  status?: string,
  isUntracked = false,
): string {
  if (!status) return "";
  if (isUntracked) return "text-green-500";
  if (status === "M") return "text-yellow-500";
  if (status === "A") return "text-green-500";
  if (status === "D") return "text-red-500";
  return "text-yellow-500"; // default for any other change
}
