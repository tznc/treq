/** Shapes returned by the gh-backed GitHub commands. */

export interface GhLabel {
  name: string;
  color: string;
}

interface GhAuthor {
  login: string;
  avatar_url?: string | null;
}

interface GhIssueComment {
  id: string;
  body: string;
  author: GhAuthor;
  created_at: string;
}

export interface GhListPage<T> {
  items: T[];
  hasMore: boolean;
  /** Cursor for the next page. Set only by cursor-paged lists (issues). */
  endCursor?: string | null;
}

export interface GhIssue {
  number: number;
  title: string;
  state: string;
  url: string;
  body: string | null;
  author: GhAuthor;
  labels: GhLabel[];
  created_at: string;
  updated_at: string;
  comments: GhIssueComment[] | null;
}

export interface GhPullRequest {
  number: number;
  title: string;
  state: string;
  url: string;
  body: string | null;
  author: GhAuthor;
  labels: GhLabel[];
  head_ref_name: string;
  base_ref_name: string;
  merge_state_status: string | null;
  created_at: string;
  updated_at: string;
  comments: GhIssueComment[] | null;
  is_draft?: boolean;
}

interface GhReviewComment {
  id: string;
  body: string;
  author: GhAuthor;
  created_at: string;
  diff_hunk: string;
  url: string;
}

/** A GitHub PR review comment thread. Read-only -- Treq never replies to or resolves these. */
export interface GhReviewThread {
  id: string;
  is_resolved: boolean;
  is_outdated: boolean;
  path: string;
  line: number | null;
  diff_side: string;
  comments: GhReviewComment[];
}
