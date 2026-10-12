import { type ClassValue, clsx } from "clsx";
import { twMerge } from "tailwind-merge";
import { convertFileSrc } from "@tauri-apps/api/core";

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

// Sanitize text for branch names.
function sanitizeForBranchName(text: string): string {
  let sanitized = text
    .toLowerCase()
    .replace(/[^a-z0-9\s-]/g, "")
    .replace(/\s+/g, "-")
    .replace(/-+/g, "-")
    .substring(0, 50) // truncate first
    .replace(/^-+|-+$/g, ""); // then trim hyphens

  // Remove trailing single-character alphabetic parts (e.g., "feat-thing-r" → "feat-thing")
  sanitized = sanitized.replace(/(-[a-z])$/, "");

  return sanitized || "unnamed";
}

/** Remove trailing single-char parts and trailing separators from a branch name */
function cleanBranchNameTrailing(name: string): string {
  let result = name;
  // Loop: strip trailing separator+single-char, then trailing separators
  let prev: string;
  do {
    prev = result;
    result = result.replace(/[/.-][a-z]$/, "");
    result = result.replace(/[/.-]+$/, "");
  } while (result !== prev);
  return result;
}

// Apply a branch-name pattern with a sanitized name.
export function applyBranchNamePattern(pattern: string, name: string): string {
  const sanitized = sanitizeForBranchName(name);
  const result = cleanBranchNameTrailing(
    pattern.replace(/\{name\}/g, sanitized),
  );
  return result || "unnamed";
}

// Extract a filename from a file path.
export function getFileName(path: string): string {
  if (!path) return "";
  // Handle both Unix (/) and Windows (\) separators
  const lastSeparator = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  return lastSeparator === -1 ? path : path.slice(lastSeparator + 1);
}

// Parse a jj timestamp into a Date; return null if invalid.
function parseTimestamp(timestamp: string): Date | null {
  try {
    // Normalize jj format to ISO-8601 before parsing.
    const iso = timestamp
      .replace(/^(\d{4}-\d{2}-\d{2})\s/, "$1T")
      .replace(/\s([+-]\d{2}:\d{2})$/, "$1");
    const date = new Date(iso);
    if (isNaN(date.getTime())) return null;
    return date;
  } catch {
    return null;
  }
}

// Format timestamp as a relative string.
export function formatRelativeTime(timestamp: string): string {
  const date = parseTimestamp(timestamp);
  if (!date) return timestamp;

  const now = new Date();
  const diffMs = now.getTime() - date.getTime();
  if (diffMs < 0) {
    const aheadSeconds = Math.floor(-diffMs / 1000);
    const aheadMinutes = Math.floor(aheadSeconds / 60);
    const aheadHours = Math.floor(aheadMinutes / 60);
    const aheadDays = Math.floor(aheadHours / 24);
    if (aheadSeconds < 60) return "in a few seconds";
    if (aheadMinutes < 60) return `in ${aheadMinutes} minutes`;
    if (aheadHours < 24) return `in ${aheadHours} hours`;
    if (aheadDays === 1) return "in 1 day";
    return `in ${aheadDays} days`;
  }
  const diffSeconds = Math.floor(diffMs / 1000);
  const diffMinutes = Math.floor(diffSeconds / 60);
  const diffHours = Math.floor(diffMinutes / 60);
  const diffDays = Math.floor(diffHours / 24);
  const diffWeeks = Math.floor(diffDays / 7);
  const diffMonths = Math.floor(diffDays / 30);
  const diffYears = Math.floor(diffDays / 365);

  if (diffSeconds < 10) return "just now";
  if (diffSeconds < 60) return `${diffSeconds} seconds ago`;
  if (diffMinutes === 1) return "1 minute ago";
  if (diffMinutes < 60) return `${diffMinutes} minutes ago`;
  if (diffHours === 1) return "1 hour ago";
  if (diffHours < 24) return `${diffHours} hours ago`;
  if (diffDays === 1) return "1 day ago";
  if (diffDays < 7) return `${diffDays} days ago`;
  if (diffWeeks === 1) return "1 week ago";
  if (diffWeeks < 4) return `${diffWeeks} weeks ago`;
  if (diffMonths === 1) return "1 month ago";
  if (diffMonths < 12) return `${diffMonths} months ago`;
  if (diffYears === 1) return "1 year ago";
  return `${diffYears} years ago`;
}

// Format timestamp for tooltip display.
export function formatFullTimestamp(timestamp: string): string {
  const date = parseTimestamp(timestamp);
  if (!date) return timestamp;

  return date.toLocaleString(undefined, {
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
    month: "long",
    second: "2-digit",
    timeZoneName: "short",
    year: "numeric",
  });
}

// Get a YYYY-MM-DD day key from a jj timestamp.
export function getDayKey(timestamp: string): string {
  const date = parseTimestamp(timestamp);
  if (!date) return "unknown";
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  return `${year}-${month}-${day}`;
}

// Format a jj timestamp into a human-friendly day label.
export function formatDayLabel(timestamp: string): string {
  const date = parseTimestamp(timestamp);
  if (!date) return timestamp;

  const now = new Date();
  const today = new Date(now.getFullYear(), now.getMonth(), now.getDate());
  const target = new Date(date.getFullYear(), date.getMonth(), date.getDate());
  const diffDays = Math.round(
    (today.getTime() - target.getTime()) / (1000 * 60 * 60 * 24),
  );

  if (diffDays === 0) return "Today";
  if (diffDays === 1) return "Yesterday";

  return date.toLocaleDateString(undefined, {
    day: "numeric",
    month: "short",
    year: "numeric",
  });
}

// Generate a stacked workspace branch name with enumeration.
export function generateStackedBranchName(
  branchPattern: string,
  parentBranch: string,
  index: number = 1,
): string {
  const baseName = `${parentBranch}-stack-${index}`;
  return applyBranchNamePattern(branchPattern, baseName);
}

/**
 * Resolve a relative image `src` from a markdown file against its base directory,
 * converting it to a Tauri asset protocol URL so the webview can load it.
 * Absolute web URLs and inline data URIs are returned unchanged.
 */
export function resolveReadmeImageSrc(
  src: string | undefined,
  baseDir: string,
): string | undefined {
  if (!src) return src;
  if (/^(https?:|data:|blob:|asset:)/i.test(src)) return src;
  const cleaned = src.replace(/^\.\//, "");
  return convertFileSrc(`${baseDir}/${cleaned}`);
}

// Construct a full workspace path; preserve legacy absolute workspace paths.
export function getFullWorkspacePath(workspace: {
  repo_path: string;
  workspace_path: string;
}): string {
  // If workspace_path is already an absolute path, return it as-is
  if (workspace.workspace_path.startsWith("/")) {
    return workspace.workspace_path;
  }
  return `${workspace.repo_path}/.treq/workspaces/${workspace.workspace_path}`;
}

/** Copy text to the system clipboard via the async Clipboard API. */
export async function copyTextToClipboard(text: string): Promise<void> {
  if (!navigator.clipboard?.writeText) {
    throw new Error("Clipboard API unavailable");
  }
  await navigator.clipboard.writeText(text);
}
