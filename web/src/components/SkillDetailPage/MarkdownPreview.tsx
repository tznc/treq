import React, { useMemo } from 'react';
import clsx from 'clsx';
import { prepareBodyHtml } from './bodyHtml';
import { renderMarkdownToHtml } from './markdown';
import styles from './styles.module.css';

// Pass `links` when the markdown is shown outside the file browser, so its
// headings sit under the page's h2 and relative links resolve on GitHub.
export function MarkdownPreview({
  content,
  links,
}: {
  content: string;
  links?: { blobBase: string; rawBase: string };
}) {
  const html = useMemo(() => {
    const rendered = renderMarkdownToHtml(content);
    return links ? prepareBodyHtml(rendered, links) : rendered;
  }, [content, links?.blobBase, links?.rawBase]);
  // The "markdown" class picks up Infima's default doc-content styling
  // (headings, tables, code blocks) so the preview matches the rest of the site.
  return (
    <div
      className={clsx('markdown', styles.markdownPreview)}
      dangerouslySetInnerHTML={{ __html: html }}
    />
  );
}
