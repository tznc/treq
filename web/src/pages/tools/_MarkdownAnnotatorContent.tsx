import React, { useState, useCallback, useRef, useEffect, useMemo } from 'react';
import { cn } from '../../lib/utils';
import { CommentInput } from '../../components/CommentInput';
import styles from './markdown-annotator.module.css';

// ── Types ─────────────────────────────────────────────────────────────────────

export interface Annotation {
  id: string;
  lineStart: number;
  lineEnd: number;
  selectedText: string;
  comment: string;
  view: 'code' | 'preview';
}

export type ViewMode = 'code' | 'preview';

interface MdBlock {
  lineStart: number;
  lineEnd: number;
  html: string;
}

// ── Helpers ───────────────────────────────────────────────────────────────────

export function uid() {
  return Math.random().toString(36).slice(2, 10);
}

export function parseGithubUrl(url: string): { rawUrl: string; displayUrl: string } | null {
  try {
    const u = new URL(url);
    if (u.hostname !== 'github.com') return null;
    const parts = u.pathname.split('/').filter(Boolean);
    if (parts.length < 5 || parts[2] !== 'blob') return null;
    const [owner, repo, , ...rest] = parts;
    const branch = rest[0];
    const filePath = rest.slice(1).join('/');
    const rawUrl = `https://raw.githubusercontent.com/${owner}/${repo}/${branch}/${filePath}`;
    return { rawUrl, displayUrl: url };
  } catch {
    return null;
  }
}

export function detectLanguage(filename: string): string {
  const ext = filename.split('.').pop()?.toLowerCase() ?? '';
  const map: Record<string, string> = {
    ts: 'typescript', tsx: 'typescript', js: 'javascript', jsx: 'javascript',
    py: 'python', rb: 'ruby', go: 'go', rs: 'rust', java: 'java',
    c: 'c', cpp: 'cpp', h: 'cpp', cs: 'csharp', php: 'php',
    sh: 'bash', bash: 'bash', zsh: 'bash', yaml: 'yaml', yml: 'yaml',
    json: 'json', toml: 'toml', md: 'markdown', mdx: 'markdown',
    html: 'html', css: 'css', scss: 'scss', sql: 'sql', xml: 'xml',
  };
  return map[ext] ?? 'text';
}

export function isMarkdown(filename: string): boolean {
  const ext = filename.split('.').pop()?.toLowerCase() ?? '';
  return ext === 'md' || ext === 'mdx';
}

export function encodeContentForUrl(content: string): string {
  const bytes = new TextEncoder().encode(content);
  let binary = '';
  bytes.forEach((b) => (binary += String.fromCharCode(b)));
  return btoa(binary).replace(/\+/g, '-').replace(/\//g, '_').replace(/=/g, '');
}

export function decodeContentFromUrl(encoded: string): string {
  const base64 = encoded.replace(/-/g, '+').replace(/_/g, '/');
  const padded = base64 + '='.repeat((4 - (base64.length % 4)) % 4);
  const binary = atob(padded);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
  return new TextDecoder().decode(bytes);
}

// ── Markdown block renderer ───────────────────────────────────────────────────

function esc(s: string): string {
  return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
}

function inlineFmt(text: string): string {
  return text
    .replace(/`([^`]+)`/g, '<code>$1</code>')
    .replace(/\*\*\*(.+?)\*\*\*/g, '<strong><em>$1</em></strong>')
    .replace(/\*\*(.+?)\*\*/g, '<strong>$1</strong>')
    .replace(/\*(.+?)\*/g, '<em>$1</em>')
    .replace(/__(.+?)__/g, '<strong>$1</strong>')
    .replace(/_(.+?)_/g, '<em>$1</em>')
    .replace(/~~(.+?)~~/g, '<del>$1</del>')
    .replace(/\[([^\]]+)\]\(([^)]+)\)/g, '<a href="$2" target="_blank" rel="noopener noreferrer">$1</a>');
}

function renderMarkdownBlocks(md: string): MdBlock[] {
  const blocks: MdBlock[] = [];
  const rawLines = md.split('\n');
  let i = 0;

  // Every non-blank line becomes its own annotatable block (one row per source
  // line, like a code diff) — only fenced code blocks span multiple lines,
  // since splitting mid-fence would break the syntax.
  while (i < rawLines.length) {
    const raw = rawLines[i];
    if (raw === undefined) break;
    const lineNum = i + 1;

    if (!raw.trim()) { i++; continue; }

    if (raw.trimStart().startsWith('```')) {
      const start = i;
      const lang = raw.replace(/^\s*```/, '').trim();
      i++;
      const codeLines: string[] = [];
      let line = rawLines[i];
      while (line !== undefined && !line.trimStart().startsWith('```')) {
        codeLines.push(esc(line));
        line = rawLines[++i];
      }
      i++;
      blocks.push({
        lineStart: start + 1,
        lineEnd: i,
        html: `<pre><code class="language-${lang || 'text'}">${codeLines.join('\n')}</code></pre>`,
      });
      continue;
    }

    const [, hashes, heading] = raw.match(/^(#{1,6})\s+(.*)/) ?? [];
    if (hashes !== undefined && heading !== undefined) {
      const level = hashes.length;
      blocks.push({ lineStart: lineNum, lineEnd: lineNum, html: `<h${level}>${inlineFmt(esc(heading))}</h${level}>` });
      i++;
      continue;
    }

    if (/^[-*_]{3,}\s*$/.test(raw)) {
      blocks.push({ lineStart: lineNum, lineEnd: lineNum, html: '<hr/>' });
      i++;
      continue;
    }

    if (raw.startsWith('>')) {
      const text = inlineFmt(esc(raw.replace(/^>\s?/, '')));
      blocks.push({ lineStart: lineNum, lineEnd: lineNum, html: `<blockquote><p>${text}</p></blockquote>` });
      i++;
      continue;
    }

    const [, item] = raw.match(/^[-*+]\s(.*)/) ?? [];
    if (item !== undefined) {
      const text = inlineFmt(esc(item));
      blocks.push({ lineStart: lineNum, lineEnd: lineNum, html: `<ul><li>${text}</li></ul>` });
      i++;
      continue;
    }

    const [, num, rest] = raw.match(/^(\d+)\.\s(.*)/) ?? [];
    if (num !== undefined && rest !== undefined) {
      const text = inlineFmt(esc(rest));
      blocks.push({ lineStart: lineNum, lineEnd: lineNum, html: `<ol start="${num}"><li>${text}</li></ol>` });
      i++;
      continue;
    }

    blocks.push({ lineStart: lineNum, lineEnd: lineNum, html: `<p>${inlineFmt(esc(raw))}</p>` });
    i++;
  }

  return blocks;
}

// ── Review prompt ─────────────────────────────────────────────────────────────

export function buildReviewPrompt(opts: {
  content: string;
  filename: string;
  sourceUrl: string | null;
  annotations: Annotation[];
  summary: string;
}): string {
  const { content, filename, sourceUrl, annotations, summary } = opts;
  const out: string[] = [];

  out.push('# File Review');
  out.push('');

  if (sourceUrl) {
    out.push(`**File:** \`${filename}\``);
    out.push(`**Source:** ${sourceUrl}`);
    out.push('');
  }

  if (annotations.length > 0) {
    out.push('## Inline Annotations');
    out.push('');
    for (const a of annotations) {
      const range = a.lineStart === a.lineEnd ? `Line ${a.lineStart}` : `Lines ${a.lineStart}–${a.lineEnd}`;
      out.push(`### ${range}`);
      if (a.selectedText) { out.push(''); out.push('```'); out.push(a.selectedText); out.push('```'); }
      out.push(''); out.push(a.comment); out.push('');
    }
  }

  if (summary.trim()) { out.push('## Summary'); out.push(''); out.push(summary.trim()); out.push(''); }

  out.push('## File Contents');
  out.push('');
  out.push('```' + (sourceUrl ? detectLanguage(filename) : 'text'));
  out.push(content);
  out.push('```');

  return out.join('\n');
}

// ── Inline annotation card (matches app's HunkLines comment card) ─────────────

interface AnnotCardProps {
  annotation: Annotation;
  onDelete: (id: string) => void;
}

// Card styling matches the app's AnnotatableDiffViewer inline comment card
// (src/components/AnnotatableDiffViewer.tsx: "border rounded-md bg-muted/60 p-2 text-[11px]").
function AnnotCard({ annotation: a, onDelete }: AnnotCardProps) {
  return (
    <div
      data-testid="inline-comment-card"
      className="group border border-border rounded-md bg-muted/60 mx-2 my-1 p-2 text-[11px]"
    >
      <div className="flex items-start gap-2">
        <div className="flex-1 min-w-0">
          <div className="font-mono text-muted-foreground mb-1">
            {a.lineStart === a.lineEnd ? `Line ${a.lineStart}` : `Lines ${a.lineStart}–${a.lineEnd}`}
          </div>
          {a.selectedText && (
            <pre className="rounded py-1 px-2 whitespace-pre-wrap break-words font-mono mb-1.5 m-0 bg-background/60 text-foreground/70">
              {a.selectedText.length > 200 ? a.selectedText.slice(0, 200) + '…' : a.selectedText}
            </pre>
          )}
          <p className="font-sans text-foreground m-0 whitespace-pre-wrap">{a.comment}</p>
        </div>
        <button
          className="opacity-0 group-hover:opacity-100 transition-opacity text-muted-foreground hover:text-destructive px-1 py-0.5 rounded hover:bg-destructive/10 flex-shrink-0"
          onClick={() => onDelete(a.id)}
          title="Delete comment"
        >
          ✕
        </button>
      </div>
    </div>
  );
}

// ── Code view ─────────────────────────────────────────────────────────────────

interface CodeViewProps {
  lines: string[];
  annotations: Annotation[];
  onAnnotationAdd: (lineStart: number, lineEnd: number, selectedText: string, comment: string) => void;
  onAnnotationDelete: (id: string) => void;
}

export function CodeView({ lines, annotations, onAnnotationAdd, onAnnotationDelete }: CodeViewProps) {
  const [anchorLine, setAnchorLine] = useState<number | null>(null);
  const [hoverLine, setHoverLine] = useState<number | null>(null);
  const [inlineForm, setInlineForm] = useState<{ lineStart: number; lineEnd: number } | null>(null);
  const hoverLineRef = useRef<number | null>(null);

  const getRange = (a: number, b: number): [number, number] => a <= b ? [a, b] : [b, a];

  const handleAddClick = (lineNum: number) => {
    if (anchorLine !== null && anchorLine !== lineNum) {
      const [start, end] = getRange(anchorLine, lineNum);
      setInlineForm({ lineStart: start, lineEnd: end });
    } else {
      setInlineForm({ lineStart: lineNum, lineEnd: lineNum });
    }
    setAnchorLine(null);
  };

  const handleLineNumClick = (lineNum: number, e: React.MouseEvent) => {
    e.stopPropagation();
    if (inlineForm) return;
    setAnchorLine((prev) => (prev === lineNum ? null : lineNum));
  };

  const handleInlineSubmit = (text: string) => {
    if (!inlineForm) return;
    const selectedText = lines.slice(inlineForm.lineStart - 1, inlineForm.lineEnd).join('\n');
    onAnnotationAdd(inlineForm.lineStart, inlineForm.lineEnd, selectedText, text);
    setInlineForm(null);
  };

  // Press "c" to comment on the hovered line — same shortcut as the app's
  // AnnotatableDiffViewer (src/components/AnnotatableDiffViewer.tsx).
  useEffect(() => {
    const handleKeydown = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement;
      const isEditable = target.tagName === 'INPUT' || target.tagName === 'TEXTAREA' || target.isContentEditable;
      if (isEditable) return;

      if (event.key.toLowerCase() === 'c' && !event.metaKey && !event.ctrlKey && !event.altKey) {
        if (!inlineForm && hoverLineRef.current !== null) {
          event.preventDefault();
          handleAddClick(hoverLineRef.current);
        }
      }
      if (event.key === 'Escape' && inlineForm) {
        event.preventDefault();
        setInlineForm(null);
      }
    };
    window.addEventListener('keydown', handleKeydown);
    return () => window.removeEventListener('keydown', handleKeydown);
  }, [inlineForm, anchorLine]);

  const [rangeStart, rangeEnd] =
    anchorLine !== null && hoverLine !== null ? getRange(anchorLine, hoverLine) : [null, null];

  const annotsByLine = new Map<number, Annotation[]>();
  for (const a of annotations) {
    const arr = annotsByLine.get(a.lineStart) ?? [];
    arr.push(a);
    annotsByLine.set(a.lineStart, arr);
  }

  const annotatedLines = new Set(annotations.flatMap((a) => {
    const out: number[] = [];
    for (let l = a.lineStart; l <= a.lineEnd; l++) out.push(l);
    return out;
  }));

  return (
    <div className="flex flex-col">
      {/*
        Grid-row layout, adapted from the app's AnnotatableDiffViewer
        (src/components/AnnotatableDiffViewer.tsx) — a fixed-width gutter
        column plus a flexible content column, instead of a <table>.
        Docusaurus/Infima applies unscoped `table { display: block; }` and
        `table tr:nth-child(2n)` striping to every <table> on the site, which
        a div/grid layout sidesteps entirely.
      */}
      <div className={cn('overflow-y-auto', styles.codeScrollArea)} style={{ maxHeight: 'calc(100vh - 220px)' }}>
        {lines.map((lineContent, idx) => {
          const lineNum = idx + 1;
          const isInRange = rangeStart !== null && rangeEnd !== null && lineNum >= rangeStart && lineNum <= rangeEnd;
          const isAnchor = lineNum === anchorLine;
          const isAnnotated = annotatedLines.has(lineNum);
          const annotsHere = annotsByLine.get(lineNum) ?? [];
          const isFormEnd = inlineForm?.lineEnd === lineNum;
          const isFormRange = inlineForm !== null && lineNum >= inlineForm.lineStart && lineNum <= inlineForm.lineEnd;

          const rowBgClass = (isInRange || isAnchor)
            ? 'bg-primary/10'
            : isFormRange
            ? 'bg-primary/5'
            : isAnnotated
            ? 'bg-amber-400/[0.06]'
            : undefined;

          return (
            <div key={lineNum} id={`annot-line-${lineNum}`}>
              <div
                className={cn(
                  'group grid gap-2 font-mono text-[13px] leading-5',
                  rowBgClass,
                )}
                style={{ gridTemplateColumns: '56px 1fr' }}
                onMouseEnter={() => { setHoverLine(lineNum); hoverLineRef.current = lineNum; }}
                onMouseLeave={() => { setHoverLine(null); hoverLineRef.current = null; }}
              >
                {/* Gutter: line number + "+" add-button, shown on row hover */}
                <div
                  className={cn('flex items-start justify-end gap-1 pl-1 pr-1 select-none', !rowBgClass && 'bg-muted/50')}
                >
                  <span
                    className={cn(
                      'text-right py-[2px] text-[12px] cursor-pointer',
                      'text-muted-foreground hover:text-foreground',
                      isAnchor && 'text-primary font-semibold',
                    )}
                    onClick={(e) => handleLineNumClick(lineNum, e)}
                    title="Click to anchor a range selection"
                  >
                    {isAnnotated && (
                      <span className="inline-block w-1.5 h-1.5 rounded-full bg-amber-400 mr-1 mb-[1px]" />
                    )}
                    {lineNum}
                  </span>
                  <button
                    type="button"
                    className={cn(
                      'flex items-center justify-center flex-shrink-0',
                      'w-4 h-5 mt-[2px]',
                      'rounded text-[11px] font-bold leading-none',
                      'text-primary-foreground bg-primary',
                      'opacity-0 group-hover:opacity-100 transition-opacity',
                      isAnchor && '!opacity-100',
                    )}
                    onClick={() => handleAddClick(lineNum)}
                    title={
                      anchorLine !== null && anchorLine !== lineNum
                        ? `Comment on lines ${Math.min(anchorLine, lineNum)}–${Math.max(anchorLine, lineNum)}`
                        : 'Add comment (or press "c" while hovering a line)'
                    }
                    tabIndex={-1}
                  >
                    +
                  </button>
                </div>

                {/* Code content */}
                <pre
                  className="m-0 py-[2px] pr-4 pl-2"
                  style={{
                    whiteSpace: 'pre-wrap',
                    wordBreak: 'break-word',
                    overflowWrap: 'anywhere',
                    background: 'transparent',
                    border: 'none',
                    fontFamily: 'inherit',
                    fontSize: 'inherit',
                    lineHeight: 'inherit',
                    fontWeight: 'normal',
                    color: 'inherit',
                  }}
                >
                  {lineContent || '​'}
                </pre>
              </div>

              {/* Inline comment form */}
              {isFormEnd && (
                <CommentInput
                  startLine={inlineForm.lineStart}
                  endLine={inlineForm.lineEnd}
                  onSubmit={handleInlineSubmit}
                  onCancel={() => setInlineForm(null)}
                />
              )}

              {/* Annotation cards */}
              {annotsHere.map((a) => (
                <AnnotCard key={a.id} annotation={a} onDelete={onAnnotationDelete} />
              ))}
            </div>
          );
        })}
      </div>
      {anchorLine !== null && (
        <div className="px-4 py-1.5 text-xs select-none border-t border-primary/20 bg-primary/5 text-primary">
          Line {anchorLine} selected — click "+" on another line to comment on a range, or click the line number to deselect
        </div>
      )}
    </div>
  );
}

// ── Preview view ──────────────────────────────────────────────────────────────

interface PreviewViewProps {
  content: string;
  annotations: Annotation[];
  onAnnotationAdd: (lineStart: number, lineEnd: number, selectedText: string, comment: string) => void;
  onAnnotationDelete: (id: string) => void;
}

export function PreviewView({ content, annotations, onAnnotationAdd, onAnnotationDelete }: PreviewViewProps) {
  const blocks = useMemo(() => renderMarkdownBlocks(content), [content]);
  const rawLines = content.split('\n');

  const [anchorLine, setAnchorLine] = useState<number | null>(null);
  const [hoverLine, setHoverLine] = useState<number | null>(null);
  const [inlineForm, setInlineForm] = useState<{ blockIdx: number; lineStart: number; lineEnd: number } | null>(null);

  const getRange = (a: number, b: number): [number, number] => a <= b ? [a, b] : [b, a];

  const handleAddClick = (block: MdBlock, blockIdx: number) => {
    if (anchorLine !== null && anchorLine !== block.lineStart) {
      const [start, end] = getRange(anchorLine, block.lineStart);
      setInlineForm({ blockIdx, lineStart: start, lineEnd: end });
    } else {
      setInlineForm({ blockIdx, lineStart: block.lineStart, lineEnd: block.lineEnd });
    }
    setAnchorLine(null);
  };

  const handleLineNumClick = (lineNum: number, e: React.MouseEvent) => {
    e.stopPropagation();
    if (inlineForm) return;
    setAnchorLine((prev) => (prev === lineNum ? null : lineNum));
  };

  const handleInlineSubmit = (text: string) => {
    if (!inlineForm) return;
    const selectedText = rawLines.slice(inlineForm.lineStart - 1, inlineForm.lineEnd).join('\n');
    onAnnotationAdd(inlineForm.lineStart, inlineForm.lineEnd, selectedText, text);
    setInlineForm(null);
  };

  const [rangeStart, rangeEnd] =
    anchorLine !== null && hoverLine !== null ? getRange(anchorLine, hoverLine) : [null, null];

  const annotsByLine = new Map<number, Annotation[]>();
  for (const a of annotations) {
    const arr = annotsByLine.get(a.lineStart) ?? [];
    arr.push(a);
    annotsByLine.set(a.lineStart, arr);
  }

  return (
    <div className="flex flex-col">
      {blocks.map((block, blockIdx) => {
        const annotsHere = annotsByLine.get(block.lineStart) ?? [];
        const isFormHere = inlineForm?.blockIdx === blockIdx;
        const isInRange = rangeStart !== null && rangeEnd !== null && block.lineStart >= rangeStart && block.lineStart <= rangeEnd;
        const isAnchor = block.lineStart === anchorLine;
        const isFormRange = inlineForm !== null && block.lineStart >= inlineForm.lineStart && block.lineStart <= inlineForm.lineEnd;

        const rowBg = (isInRange || isAnchor)
          ? 'hsl(var(--primary)/0.1)'
          : isFormRange
          ? 'hsl(var(--primary)/0.05)'
          : undefined;

        return (
          <React.Fragment key={blockIdx}>
            {/* Line row: [line#] [+btn] [content] */}
            <div
              className="group flex items-start py-0.5 rounded-sm"
              style={{ background: rowBg }}
              onMouseEnter={() => setHoverLine(block.lineStart)}
              onMouseLeave={() => setHoverLine(null)}
            >
              {/* Gutter: line number + add-button, button sits right of the number */}
              <div className="w-14 flex-shrink-0 flex items-start justify-end gap-1 pt-1 pr-1 select-none">
                <span
                  className={cn(
                    'text-right text-xs font-mono cursor-pointer text-muted-foreground hover:text-foreground',
                    isAnchor && 'text-primary font-semibold',
                  )}
                  onClick={(e) => handleLineNumClick(block.lineStart, e)}
                  title="Click to anchor a range selection"
                >
                  {block.lineStart}
                  {block.lineEnd > block.lineStart && <span className="text-muted-foreground/60">–{block.lineEnd}</span>}
                </span>
                <button
                  className={cn(
                    'flex items-center justify-center flex-shrink-0 w-4 h-4 mt-[1px]',
                    'rounded text-[11px] font-bold leading-none text-primary-foreground bg-primary',
                    'opacity-0 group-hover:opacity-100 transition-opacity',
                    isAnchor && '!opacity-100',
                  )}
                  onClick={() => handleAddClick(block, blockIdx)}
                  title={
                    anchorLine !== null && anchorLine !== block.lineStart
                      ? `Comment on lines ${Math.min(anchorLine, block.lineStart)}–${Math.max(anchorLine, block.lineStart)}`
                      : 'Add comment'
                  }
                  tabIndex={-1}
                >
                  +
                </button>
              </div>

              {/* Rendered markdown block */}
              <div
                className={styles.previewBlockContent}
                dangerouslySetInnerHTML={{ __html: block.html }}
              />
            </div>

            {/* Inline comment form */}
            {isFormHere && (
              <div className="flex">
                <div className="w-14 flex-shrink-0" />
                <div className="flex-1 min-w-0 pr-4">
                  <CommentInput
                    startLine={inlineForm.lineStart}
                    endLine={inlineForm.lineEnd}
                    onSubmit={handleInlineSubmit}
                    onCancel={() => setInlineForm(null)}
                  />
                </div>
              </div>
            )}

            {/* Annotation cards */}
            {annotsHere.map((a) => (
              <div key={a.id} className="flex">
                <div className="w-14 flex-shrink-0" />
                <div className="flex-1 min-w-0 pr-4 py-1">
                  <AnnotCard annotation={a} onDelete={onAnnotationDelete} />
                </div>
              </div>
            ))}
          </React.Fragment>
        );
      })}
      {anchorLine !== null && (
        <div
          className="px-4 py-1.5 text-xs select-none border-t"
          style={{
            color: 'hsl(var(--primary))',
            background: 'hsl(var(--primary)/0.05)',
            borderColor: 'hsl(var(--primary)/0.2)',
          }}
        >
          Line {anchorLine} selected — click "+" on another line to comment on a range, or click the line number to deselect
        </div>
      )}
    </div>
  );
}

// ── File loader ───────────────────────────────────────────────────────────────

interface FileLoaderProps {
  onLoad: (content: string, filename: string, sourceUrl: string | null) => void;
}

export function FileLoader({ onLoad }: FileLoaderProps) {
  const [urlInput, setUrlInput] = useState('');
  const [pasteContent, setPasteContent] = useState('');
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const fetchFromGithub = async () => {
    if (!urlInput.trim()) return;
    setError(null);
    const parsed = parseGithubUrl(urlInput.trim());
    if (!parsed) {
      setError('Invalid GitHub URL. Expected: github.com/owner/repo/blob/branch/path/to/file');
      return;
    }
    setLoading(true);
    try {
      const res = await fetch(parsed.rawUrl);
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      const text = await res.text();
      const filename = parsed.rawUrl.split('/').pop() ?? 'file';
      onLoad(text, filename, parsed.displayUrl);
    } catch (e) {
      setError(e instanceof Error ? `Failed to fetch: ${e.message}` : 'Failed to fetch');
    } finally {
      setLoading(false);
    }
  };

  const loadPasted = () => {
    if (!pasteContent.trim()) { setError('No content to load.'); return; }
    onLoad(pasteContent, '', null);
  };

  return (
    <div className={styles.loader}>
      {/* GitHub URL */}
      <div className={styles.loaderSection}>
        <label className={styles.loaderLabel}>GitHub file URL</label>
        <div className={styles.loaderRow}>
          <input
            className={styles.loaderInput}
            type="url"
            placeholder="https://github.com/owner/repo/blob/main/README.md"
            value={urlInput}
            onChange={(e) => { setUrlInput(e.target.value); setError(null); }}
            onKeyDown={(e) => { if (e.key === 'Enter') fetchFromGithub(); }}
          />
          <button className="button button--primary button--sm" onClick={fetchFromGithub} disabled={loading || !urlInput.trim()}>
            {loading ? 'Loading…' : 'Fetch'}
          </button>
        </div>
      </div>

      <div className={styles.loaderDivider}>
        <span>or paste content below</span>
      </div>

      {/* Paste */}
      <div className={styles.loaderSection}>
        <textarea
          className={styles.loaderTextarea}
          placeholder="Paste file contents here…"
          value={pasteContent}
          onChange={(e) => { setPasteContent(e.target.value); setError(null); }}
          rows={10}
        />
        <button
          className="button button--primary button--sm"
          onClick={loadPasted}
          disabled={!pasteContent.trim()}
          style={{ alignSelf: 'flex-start' }}
        >
          Load
        </button>
      </div>

      {error && <div className={styles.loaderError}>{error}</div>}
    </div>
  );
}

// ── Review panel ──────────────────────────────────────────────────────────────

interface ReviewPanelProps {
  content: string;
  filename: string;
  sourceUrl: string | null;
  annotations: Annotation[];
  onBack: () => void;
}

export function ReviewPanel({ content, filename, sourceUrl, annotations, onBack }: ReviewPanelProps) {
  const [summary, setSummary] = useState('');
  const [copied, setCopied] = useState(false);

  const prompt = buildReviewPrompt({ content, filename, sourceUrl, annotations, summary });
  const claudeUrl = `https://claude.ai/new?q=${encodeURIComponent(prompt)}`;
  const chatgptUrl = `https://chatgpt.com/?q=${encodeURIComponent(prompt)}`;

  const handleCopy = async () => {
    await navigator.clipboard.writeText(prompt);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  return (
    <div className={styles.reviewPanel}>
      <div className={styles.reviewPanelHeader}>
        <span className={styles.reviewPanelTitle}>Finish Review</span>
      </div>
      <div className={styles.reviewStats}>
        <span className={styles.reviewStat}>{annotations.length} annotation{annotations.length !== 1 ? 's' : ''}</span>
        <span className={styles.reviewStat}>{content.split('\n').length} lines</span>
        {sourceUrl && (
          <a className={styles.reviewSourceLink} href={sourceUrl} target="_blank" rel="noopener noreferrer">View source</a>
        )}
      </div>
      <div className={styles.reviewSummarySection}>
        <label className={styles.reviewLabel}>Summary (optional)</label>
        <textarea
          className={styles.reviewSummary}
          placeholder="Add overall review notes, context, or questions…"
          value={summary}
          onChange={(e) => setSummary(e.target.value)}
          rows={4}
        />
      </div>
      <div className={styles.reviewActions}>
        <button className="button button--secondary button--sm" onClick={handleCopy}>{copied ? '✓ Copied!' : 'Copy prompt'}</button>
        <a className={styles.reviewOpenClaude} href={claudeUrl} target="_blank" rel="noopener noreferrer">Open in Claude</a>
        <a className={styles.reviewOpenChatgpt} href={chatgptUrl} target="_blank" rel="noopener noreferrer">Open in ChatGPT</a>
      </div>
      <div className={styles.reviewPromptPreview}>
        <div className={styles.reviewPromptHeader}><span className={styles.reviewPromptLabel}>Prompt preview</span></div>
        <pre className={styles.reviewPromptText}>{prompt}</pre>
      </div>
      <button className="button button--secondary button--sm" style={{ alignSelf: 'flex-start' }} onClick={onBack}>← Back to annotations</button>
    </div>
  );
}

// ── Embed panel ───────────────────────────────────────────────────────────────

const EMBED_BASE = 'https://treq.dev/tools/markdown-annotator-embed';
const CONTENT_SIZE_LIMIT = 60_000;

interface EmbedPanelProps {
  content: string;
  filename: string;
  sourceUrl: string | null;
  onClose: () => void;
}

function EmbedPanel({ content, filename, sourceUrl, onClose }: EmbedPanelProps) {
  const [width, setWidth] = useState(900);
  const [height, setHeight] = useState(650);
  const [copied, setCopied] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);

  const tooLarge = !sourceUrl && content.length > CONTENT_SIZE_LIMIT;

  const embedUrl = sourceUrl
    ? `${EMBED_BASE}?url=${encodeURIComponent(sourceUrl)}`
    : tooLarge
    ? null
    : `${EMBED_BASE}?content=${encodeContentForUrl(content)}&filename=${encodeURIComponent(filename || 'file')}`;

  const iframeCode = embedUrl
    ? `<iframe\n  src="${embedUrl}"\n  width="${width}"\n  height="${height}"\n  frameborder="0"\n  style="border-radius:8px;border:1px solid #e5e7eb;"\n></iframe>`
    : null;

  const handleCopy = async () => {
    if (!iframeCode) return;
    await navigator.clipboard.writeText(iframeCode);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  return (
    <div className={styles.embedPanel}>
      <div className={styles.embedPanelHeader}>
        <div className={styles.embedPanelTitleRow}>
          <span className={styles.embedPanelTitle}>Embed this file</span>
          <button className={styles.embedPanelClose} onClick={onClose}>✕</button>
        </div>
        <p className={styles.embedPanelDesc}>
          Copy the code below and paste it into any HTML page to embed this file annotator.
        </p>
      </div>

      {tooLarge && (
        <div className={styles.embedWarning}>
          Content is too large to encode in a URL ({(content.length / 1024).toFixed(0)} KB). Use a GitHub URL to enable embedding.
        </div>
      )}

      {!tooLarge && embedUrl && (
        <>
          <div className={styles.embedSizeRow}>
            <label className={styles.embedSizeLabel}>
              Width
              <input className={styles.embedSizeInput} type="number" value={width} min={320} max={2560}
                onChange={(e) => setWidth(Math.max(320, Number(e.target.value)))} />
              <span className={styles.embedSizeUnit}>px</span>
            </label>
            <span className={styles.embedSizeSep}>×</span>
            <label className={styles.embedSizeLabel}>
              Height
              <input className={styles.embedSizeInput} type="number" value={height} min={300} max={2000}
                onChange={(e) => setHeight(Math.max(300, Number(e.target.value)))} />
              <span className={styles.embedSizeUnit}>px</span>
            </label>
          </div>

          <div className={styles.embedCodeSection}>
            <div className={styles.embedCodeLabel}>Embed code</div>
            <div className={styles.embedCodeRow}>
              <input
                ref={inputRef}
                className={styles.embedCodeInput}
                type="text"
                readOnly
                value={iframeCode ?? ''}
                onClick={() => inputRef.current?.select()}
              />
              <button className="button button--primary button--sm" onClick={handleCopy}>
                {copied ? '✓ Copied' : 'Copy'}
              </button>
            </div>
          </div>

          <div className={styles.embedPreviewRow}>
            <a className={styles.embedPreviewLink} href={embedUrl} target="_blank" rel="noopener noreferrer">
              Preview embed ↗
            </a>
          </div>

          <div className={styles.embedInstructions}>
            <div className={styles.embedInstructionsTitle}>How to embed</div>
            <ol className={styles.embedInstructionsList}>
              <li>Copy the embed code above.</li>
              <li>Paste it into any HTML page where you want the annotator to appear.</li>
              <li>Adjust <code>width</code> and <code>height</code> to fit your layout — <code>width="100%"</code> works for responsive layouts.</li>
              <li>Viewers can annotate the file and export their review to Claude or ChatGPT directly from the embed.</li>
            </ol>
          </div>
        </>
      )}
    </div>
  );
}

// ── Annotator tool ────────────────────────────────────────────────────────────

interface InitialFileState {
  content: string;
  filename: string;
  sourceUrl: string | null;
}

interface AnnotatorToolProps {
  compact?: boolean;
  initialFile?: InitialFileState;
}

export function AnnotatorTool({ compact = false, initialFile }: AnnotatorToolProps) {
  const [content, setContent] = useState<string | null>(initialFile?.content ?? null);
  const [filename, setFilename] = useState(initialFile?.filename ?? '');
  const [sourceUrl, setSourceUrl] = useState<string | null>(initialFile?.sourceUrl ?? null);
  const [viewMode, setViewMode] = useState<ViewMode>(() =>
    initialFile ? (isMarkdown(initialFile.filename) ? 'preview' : 'code') : 'code'
  );
  const [annotations, setAnnotations] = useState<Annotation[]>([]);
  const [showReview, setShowReview] = useState(false);
  const [showEmbed, setShowEmbed] = useState(false);

  const lines = content?.split('\n') ?? [];

  const handleLoad = useCallback((c: string, fn: string, url: string | null) => {
    setContent(c);
    setFilename(fn);
    setSourceUrl(url);
    setAnnotations([]);
    setViewMode(isMarkdown(fn) ? 'preview' : 'code');
    setShowReview(false);
    setShowEmbed(false);
  }, []);

  const handleAnnotationAdd = useCallback(
    (lineStart: number, lineEnd: number, selectedText: string, comment: string) => {
      setAnnotations((prev) => [
        ...prev,
        { id: uid(), lineStart, lineEnd, selectedText, comment, view: viewMode },
      ]);
    },
    [viewMode]
  );

  const handleAnnotationDelete = useCallback((id: string) => {
    setAnnotations((prev) => prev.filter((a) => a.id !== id));
  }, []);

  const handleClear = useCallback(() => {
    if (compact) return;
    setContent(null);
    setFilename('');
    setSourceUrl(null);
    setAnnotations([]);
    setShowReview(false);
    setShowEmbed(false);
  }, [compact]);

  if (!content) {
    return (
      <div className={styles.toolRoot}>
        <div className={styles.toolHeader}>
          {!compact && (
            <div className={styles.breadcrumb}>
              <a href="/tools">Tools</a>
              <span> / </span>
              <span>Markdown Annotator</span>
            </div>
          )}
          <h1 className={styles.toolTitle}>Markdown Annotator</h1>
          <p className={styles.toolSubtitle}>
            Annotate markdown and code files like a PR review. Add inline comments, then export a review prompt for Claude or ChatGPT.
          </p>
        </div>
        <FileLoader onLoad={handleLoad} />
      </div>
    );
  }

  if (showReview) {
    return (
      <div className={styles.toolRoot}>
        <div className={styles.toolHeader}>
          {!compact && (
            <div className={styles.breadcrumb}>
              <a href="/tools">Tools</a>
              <span> / </span>
              <button className={styles.breadcrumbBtn} onClick={() => setShowReview(false)}>Markdown Annotator</button>
              <span> / </span>
              <span>Review</span>
            </div>
          )}
          <h1 className={styles.toolTitle}>Review{filename ? `: ${filename}` : ''}</h1>
        </div>
        <ReviewPanel
          content={content}
          filename={filename}
          sourceUrl={sourceUrl}
          annotations={annotations}
          onBack={() => setShowReview(false)}
        />
      </div>
    );
  }

  return (
    <div className={styles.toolRoot}>
      <div className={styles.toolHeader}>
        {!compact && (
          <div className={styles.breadcrumb}>
            <a href="/tools">Tools</a>
            <span> / </span>
            <span>Markdown Annotator</span>
          </div>
        )}
        <div className={styles.toolTitleRow}>
          <h1 className={compact ? styles.toolTitleCompact : styles.toolTitle}>
            {filename || 'Untitled'}
          </h1>
          <div className={styles.toolHeaderActions}>
            {!compact && (
              <>
                <button
                  className={`button button--sm ${showEmbed ? 'button--primary' : 'button--secondary'}`}
                  onClick={() => setShowEmbed((v) => !v)}
                >
                  {showEmbed ? 'Hide embed' : '‹/› Embed'}
                </button>
                <button className="button button--secondary button--sm" onClick={handleClear}>Change file</button>
              </>
            )}
            <button className="button button--primary button--sm" onClick={() => setShowReview(true)}>
              Finish review ({annotations.length})
            </button>
          </div>
        </div>
        {sourceUrl && (
          <div className={styles.sourceUrlBar}>
            <a href={sourceUrl} target="_blank" rel="noopener noreferrer" className={styles.sourceUrlLink}>{sourceUrl}</a>
          </div>
        )}
      </div>

      {showEmbed && !compact && (
        <EmbedPanel content={content} filename={filename} sourceUrl={sourceUrl} onClose={() => setShowEmbed(false)} />
      )}

      <div className={styles.toolBody}>
        {/* GitHub-style Code/Preview tabs at the top of the review container — markdown is always assumed */}
        <div className={styles.bodyTabs}>
          <button
            className={cn(styles.bodyTab, viewMode === 'code' && styles.bodyTabActive)}
            onClick={() => setViewMode('code')}
          >
            Code
          </button>
          <button
            className={cn(styles.bodyTab, viewMode === 'preview' && styles.bodyTabActive)}
            onClick={() => setViewMode('preview')}
          >
            Preview
          </button>
        </div>
        {viewMode === 'code' ? (
          <CodeView
            lines={lines}
            annotations={annotations}
            onAnnotationAdd={handleAnnotationAdd}
            onAnnotationDelete={handleAnnotationDelete}
          />
        ) : (
          <PreviewView
            content={content}
            annotations={annotations}
            onAnnotationAdd={handleAnnotationAdd}
            onAnnotationDelete={handleAnnotationDelete}
          />
        )}
      </div>
    </div>
  );
}
