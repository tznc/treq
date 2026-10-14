import { createContext, useContext, useRef, useState } from "react";
import {
  CheckCircle2,
  ChevronDown,
  ChevronRight,
  Github,
  Quote,
} from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";
import type { GhReviewThread } from "../../lib/api-types";
import { SelectionQuoteToolbar } from "./SelectionQuoteToolbar";

/**
 * Posts a reply to a GitHub review thread. Provided only by screens that can
 * write to the PR (the PR review screen); without it threads stay read-only.
 */
export const GithubThreadReplyContext = createContext<
  ((threadId: string, body: string) => Promise<void>) | null
>(null);

export interface GithubQuote {
  text: string;
  author: string;
  avatarUrl?: string;
  commentUrl: string;
}

interface GithubCommentCardProps {
  thread: GhReviewThread;
  collapsed: boolean;
  onToggleCollapse: () => void;
  onQuote: (quote: GithubQuote) => void;
}

/** Format a GitHub comment timestamp; omit the year when it is the current year. */
export function formatGithubCommentDate(iso: string): string {
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return iso;
  const sameYear = date.getFullYear() === new Date().getFullYear();
  return date.toLocaleDateString(undefined, {
    ...(sameYear ? {} : { year: "numeric" as const }),
    month: "short",
    day: "numeric",
  });
}

function AuthorAvatar({
  login,
  avatarUrl,
  sizeClass = "w-8 h-8",
}: {
  login: string;
  avatarUrl: string | null | undefined;
  sizeClass?: string;
}) {
  if (avatarUrl) {
    return (
      <img
        src={avatarUrl}
        alt={login}
        className={`${sizeClass} rounded-full flex-shrink-0`}
      />
    );
  }
  return (
    <div
      className={`${sizeClass} rounded-full bg-muted flex items-center justify-center text-xs font-medium text-muted-foreground flex-shrink-0`}
    >
      {login.slice(0, 1).toUpperCase()}
    </div>
  );
}

function AuthorProfileLink({
  login,
  className,
}: {
  login: string;
  className?: string;
}) {
  const profileUrl = `https://github.com/${login}`;
  return (
    <a
      href={profileUrl}
      className={className}
      onClick={(event) => {
        event.preventDefault();
        event.stopPropagation();
        void openUrl(profileUrl);
      }}
    >
      @{login}
    </a>
  );
}

/** Read-only GitHub PR review comment thread, rendered inline in the diff. */
export function GithubCommentCard({
  thread,
  collapsed,
  onToggleCollapse,
  onQuote,
}: GithubCommentCardProps) {
  const [firstComment] = thread.comments;

  return (
    <div className="rounded-md border border-sky-500/30 bg-sky-500/5 overflow-hidden">
      <button
        className="w-full flex items-center gap-2 px-3 py-1.5 text-left hover:bg-sky-500/10 font-sans min-w-0"
        onClick={onToggleCollapse}
        data-testid="github-thread-toggle"
      >
        {collapsed ? (
          <ChevronRight className="w-3 h-3 text-muted-foreground flex-shrink-0" />
        ) : (
          <ChevronDown className="w-3 h-3 text-muted-foreground flex-shrink-0" />
        )}
        {collapsed && firstComment && (
          <>
            <AuthorAvatar
              login={firstComment.author.login}
              avatarUrl={firstComment.author.avatar_url}
              sizeClass="w-5 h-5"
            />
            <AuthorProfileLink
              login={firstComment.author.login}
              className="text-xs font-medium font-sans hover:underline text-foreground flex-shrink-0"
            />
          </>
        )}
        {thread.is_resolved && (
          <span className="flex items-center gap-1 px-1.5 py-0.5 rounded bg-muted text-muted-foreground text-[10px] font-medium flex-shrink-0">
            <CheckCircle2 className="w-3 h-3" />
            Resolved
          </span>
        )}
        {collapsed && firstComment && (
          <span
            className="text-xs text-muted-foreground/70 truncate min-w-0 font-sans"
            data-testid="github-thread-collapsed-preview"
          >
            {firstComment.body.replace(/\s+/g, " ").trim()}
          </span>
        )}
        {!collapsed && (
          <span className="text-xs text-muted-foreground">
            {thread.comments.length} comment
            {thread.comments.length !== 1 ? "s" : ""}
          </span>
        )}
      </button>
      {!collapsed && (
        <div className="px-3 pb-3 space-y-2">
          {thread.comments.map((comment) => (
            <GithubCommentBody
              key={comment.id}
              comment={comment}
              onQuote={onQuote}
            />
          ))}
          <ThreadReply threadId={thread.id} />
        </div>
      )}
    </div>
  );
}

function ThreadReply({ threadId }: { threadId: string }) {
  const reply = useContext(GithubThreadReplyContext);
  const [open, setOpen] = useState(false);
  const [text, setText] = useState("");
  const [sending, setSending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  if (!reply) return null;

  if (!open) {
    return (
      <button
        className="text-xs text-muted-foreground hover:text-foreground font-sans"
        onClick={() => setOpen(true)}
      >
        Reply
      </button>
    );
  }

  const send = async () => {
    if (!text.trim()) return;
    setSending(true);
    setError(null);
    try {
      await reply(threadId, text.trim());
      setText("");
      setOpen(false);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setSending(false);
    }
  };

  return (
    <div className="space-y-1.5 font-sans">
      <textarea
        autoFocus
        value={text}
        onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) void send();
          if (e.key === "Escape") setOpen(false);
        }}
        placeholder="Reply on GitHub…"
        className="w-full min-h-[60px] rounded-md border border-border bg-background px-2 py-1.5 text-xs"
      />
      {error && <p className="text-xs text-destructive">{error}</p>}
      <div className="flex justify-end gap-2">
        <button
          className="text-xs text-muted-foreground hover:text-foreground"
          onClick={() => setOpen(false)}
        >
          Cancel
        </button>
        <button
          className="text-xs rounded bg-primary text-primary-foreground px-2 py-1 disabled:opacity-50"
          disabled={sending || !text.trim()}
          onClick={() => void send()}
        >
          Send reply
        </button>
      </div>
    </div>
  );
}

function GithubCommentBody({
  comment,
  onQuote,
}: {
  comment: GhReviewThread["comments"][number];
  onQuote: (quote: GithubQuote) => void;
}) {
  const bodyRef = useRef<HTMLDivElement>(null);

  const quoteComment = (text: string) =>
    onQuote({
      text,
      author: comment.author.login,
      avatarUrl: comment.author.avatar_url ?? undefined,
      commentUrl: comment.url,
    });

  return (
    <div
      className="group relative bg-background rounded-md p-2.5 pr-9 border border-border/60 font-sans"
      data-testid="github-comment-card"
    >
      <div className="flex items-center gap-2 mb-1.5">
        <AuthorAvatar
          login={comment.author.login}
          avatarUrl={comment.author.avatar_url}
        />
        <AuthorProfileLink
          login={comment.author.login}
          className="text-xs font-medium font-sans hover:underline text-foreground"
        />
        <span className="text-xs text-muted-foreground font-sans">
          {formatGithubCommentDate(comment.created_at)}
        </span>
        <Github
          className="w-3.5 h-3.5 text-sky-600 dark:text-sky-400 flex-shrink-0"
          aria-label="GitHub"
        />
      </div>
      <div
        ref={bodyRef}
        className="text-sm whitespace-pre-wrap select-text font-sans"
        data-testid="github-comment-body"
      >
        {comment.body}
      </div>
      <button
        className="absolute top-2 right-2 p-1 rounded hover:bg-accent text-muted-foreground hover:text-foreground opacity-0 group-hover:opacity-100 transition-opacity font-sans"
        onClick={() => quoteComment(comment.body)}
        title="Quote this comment"
      >
        <Quote className="w-3.5 h-3.5" />
      </button>
      <SelectionQuoteToolbar containerRef={bodyRef} onQuote={quoteComment} />
    </div>
  );
}
