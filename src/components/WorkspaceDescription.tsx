import { MarkdownContent } from "./MarkdownContent";

// Block syntax at a line start (heading, list, quote, fence, table row) or
// inline emphasis, code or a link anywhere.
const MARKDOWN_PATTERNS = [
  /^#{1,6}\s/m,
  /^\s*[-*+]\s/m,
  /^\s*\d+\.\s/m,
  /^>\s/m,
  /^```/m,
  /^\|.*\|\s*$/m,
  /\*\*[^*\n]+\*\*/,
  /`[^`\n]+`/,
  /\[[^\]\n]+\]\([^)\s]+\)/,
];

export function looksLikeMarkdown(text: string): boolean {
  return MARKDOWN_PATTERNS.some((pattern) => pattern.test(text));
}

/** The workspace description, which Treq also uses as the PR body. */
export function WorkspaceDescription({ description }: { description: string }) {
  return (
    <div
      data-testid="workspace-description"
      className="rounded-lg border border-border px-4 py-3"
    >
      {looksLikeMarkdown(description) ? (
        <MarkdownContent content={description} className="prose-sm" />
      ) : (
        <p className="text-sm text-foreground whitespace-pre-wrap">
          {description}
        </p>
      )}
    </div>
  );
}
