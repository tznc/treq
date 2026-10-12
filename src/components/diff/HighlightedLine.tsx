import type React from "react";
import { highlightCode } from "../../lib/syntax-highlight";
import { highlightInHtml } from "../../lib/text-search";

export interface HighlightedLineProps {
  content: string;
  language: string | null;
  searchQuery?: string;
  searchHighlightOffset?: number;
}

// Syntax-highlighted source for one diff line, with optional search-match
// marks. Shared by the Changes and Commits diff viewers.
export const HighlightedLine: React.FC<HighlightedLineProps> = ({
  content,
  language,
  searchQuery,
  searchHighlightOffset,
}) => {
  const highlighted = highlightCode(content, language);
  const { html } = searchQuery
    ? highlightInHtml(highlighted, searchQuery, searchHighlightOffset ?? -1)
    : { html: highlighted };
  // biome-ignore lint/security/noDangerouslySetInnerHtml: Prism escapes source text before adding token markup.
  return <span dangerouslySetInnerHTML={{ __html: html }} />;
};
HighlightedLine.displayName = "HighlightedLine";
