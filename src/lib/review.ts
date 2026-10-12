import type {
  ConflictRegion,
  LineComment as ApiLineComment,
} from "./api-types";

/** Sentinel hunkId marking a comment as attached to a whole file rather than a specific line. */
export const FILE_COMMENT_HUNK_ID = "__file__";

export interface LineComment {
  id: string;
  filePath: string;
  hunkId: string;
  startLine: number;
  endLine: number;
  lineContent: string[];
  text: string;
  createdAt: string;
  lineSide?: "old" | "new";
  /** Set when this comment was seeded by quoting a GitHub review comment. */
  source?: "github";
  githubAuthor?: string;
  githubAvatarUrl?: string;
  githubCommentUrl?: string;
}

export interface PendingComment {
  filePath: string;
  hunkId: string;
  displayAtLineIndex: number;
  startLine: number;
  endLine: number;
  lineContent: string[];
  lineSide: "old" | "new";
  /** Set when this pending comment was seeded by quoting a GitHub review comment. */
  githubMeta?: {
    author: string;
    avatarUrl?: string;
    commentUrl?: string;
  };
}

export function toLocalLineComment(c: ApiLineComment): LineComment {
  return {
    id: c.id,
    filePath: c.file_path,
    hunkId: c.hunk_id,
    startLine: c.start_line,
    endLine: c.end_line,
    lineContent: c.line_content,
    text: c.text,
    createdAt: c.created_at,
    lineSide: c.line_side,
    source: c.source,
    githubAuthor: c.github_author,
    githubAvatarUrl: c.github_avatar_url,
    githubCommentUrl: c.github_comment_url,
  };
}

export function toApiLineComment(c: LineComment): ApiLineComment {
  return {
    id: c.id,
    file_path: c.filePath,
    hunk_id: c.hunkId,
    start_line: c.startLine,
    end_line: c.endLine,
    line_content: c.lineContent,
    text: c.text,
    created_at: c.createdAt,
    line_side: c.lineSide,
    source: c.source,
    github_author: c.githubAuthor,
    github_avatar_url: c.githubAvatarUrl,
    github_comment_url: c.githubCommentUrl,
  };
}

function formatCommentMarkdown(comment: LineComment, filePath: string): string {
  if (comment.hunkId === FILE_COMMENT_HUNK_ID) {
    return `${filePath}\n> ${comment.text}\n\n`;
  }
  const lineRef =
    comment.startLine === comment.endLine
      ? `${filePath}:${comment.startLine}`
      : `${filePath}:${comment.startLine}:${comment.endLine}`;
  let block = `${lineRef}\n`;
  if (comment.source === "github") {
    block += `> Quoting GitHub review comment by @${comment.githubAuthor}:\n`;
    block += `> ${comment.lineContent.join("\n> ")}\n\n`;
  } else {
    block += "```\n";
    block += `${comment.lineContent.join("\n")}\n`;
    block += "```\n";
  }
  block += `> ${comment.text}\n\n`;
  return block;
}

interface ConflictCommentInput {
  filePath: string;
  conflictNumber: number;
  text: string;
}

export interface FormatReviewMarkdownParams {
  comments: LineComment[];
  finalReviewComment: string;
  conflictComments?: Map<string, ConflictCommentInput>;
  conflictRegionsByFile?: Map<string, ConflictRegion[]>;
  actualConflictedFiles?: string[];
}

/** Builds the markdown prompt sent to an agent from an in-progress review's comments/summary. */
export function formatReviewMarkdown({
  comments,
  finalReviewComment,
  conflictComments,
  conflictRegionsByFile,
  actualConflictedFiles = [],
}: FormatReviewMarkdownParams): string {
  let markdown = "";
  if (conflictComments && conflictComments.size > 0) {
    markdown += "## Conflict Resolution\n\n";
    for (const comment of conflictComments.values()) {
      if (comment.text.trim()) {
        const regions = conflictRegionsByFile?.get(comment.filePath);
        const region = regions?.find(
          (r) => r.conflict_number === comment.conflictNumber,
        );
        let header = `### ${comment.filePath} - Conflict ${comment.conflictNumber}`;
        if (region)
          header += ` (lines ${region.start_line}-${region.end_line})`;
        markdown += `${header}\n`;
        markdown += `> ${comment.text}\n\n`;
      }
    }
  }
  if (
    comments.length > 0 ||
    finalReviewComment.trim() ||
    actualConflictedFiles.length > 0
  ) {
    markdown += "## Code Review\n\n";
    if (actualConflictedFiles.length > 0) {
      markdown += "### Conflicted Files\n\n";
      for (const filePath of actualConflictedFiles)
        markdown += `- ${filePath}\n`;
      markdown += "\n";
    }
    if (finalReviewComment.trim()) {
      markdown += "### Summary\n";
      markdown += `${finalReviewComment.trim()}\n\n`;
    }
    if (comments.length > 0) {
      markdown += "### Comments\n\n";
      const commentsByFile = comments.reduce(
        (acc, comment) => {
          if (!acc[comment.filePath]) acc[comment.filePath] = [];
          acc[comment.filePath].push(comment);
          return acc;
        },
        {} as Record<string, LineComment[]>,
      );
      for (const [filePath, fileComments] of Object.entries(commentsByFile)) {
        for (const comment of fileComments) {
          markdown += formatCommentMarkdown(comment, filePath);
        }
      }
    }
  }
  return markdown;
}
