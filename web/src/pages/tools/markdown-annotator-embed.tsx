import React, { useState, useEffect, lazy, Suspense } from 'react';
import Head from '@docusaurus/Head';
import BrowserOnly from '@docusaurus/BrowserOnly';
import styles from './markdown-annotator-embed.module.css';

const AnnotatorTool = lazy(() =>
  import('./_MarkdownAnnotatorContent').then((m) => ({ default: m.AnnotatorTool }))
);

// ── Treq badge ────────────────────────────────────────────────────────────────

function TreqBadge() {
  return (
    <a
      href="https://treq.dev/?utm_source=treq-embed&utm_medium=badge&utm_campaign=markdown-annotator"
      target="_blank"
      rel="noopener noreferrer"
      className={styles.badge}
      title="Made with Treq"
    >
      <img src="/img/favicon.svg" alt="Treq" className={styles.badgeLogo} />
      <span className={styles.badgeText}>treq.dev</span>
    </a>
  );
}

// ── Embed loader ──────────────────────────────────────────────────────────────

interface LoadedFile {
  content: string;
  filename: string;
  sourceUrl: string | null;
}

function EmbedLoader({ onLoad, onError }: { onLoad: (f: LoadedFile) => void; onError: (msg: string) => void }) {
  useEffect(() => {
    const params = new URLSearchParams(window.location.search);
    const urlParam = params.get('url');
    const contentParam = params.get('content');
    const filenameParam = params.get('filename') ?? 'untitled.md';

    if (urlParam) {
      let rawUrl: string;
      let displayUrl = urlParam;

      try {
        const u = new URL(urlParam);
        if (u.hostname === 'github.com') {
          const parts = u.pathname.split('/').filter(Boolean);
          if (parts.length >= 5 && parts[2] === 'blob') {
            const [owner, repo, , ...rest] = parts;
            const branch = rest[0];
            const filePath = rest.slice(1).join('/');
            rawUrl = `https://raw.githubusercontent.com/${owner}/${repo}/${branch}/${filePath}`;
          } else {
            onError('Invalid GitHub URL format. Expected: github.com/owner/repo/blob/branch/path');
            return;
          }
        } else if (u.hostname === 'raw.githubusercontent.com') {
          rawUrl = urlParam;
        } else {
          onError('Only github.com URLs are supported.');
          return;
        }
      } catch {
        onError('Invalid URL.');
        return;
      }

      fetch(rawUrl)
        .then((r) => {
          if (!r.ok) throw new Error(`HTTP ${r.status}`);
          return r.text();
        })
        .then((text) => {
          const filename = rawUrl.split('/').pop() ?? filenameParam;
          onLoad({ content: text, filename, sourceUrl: displayUrl });
        })
        .catch((e) => onError(`Failed to fetch file: ${e instanceof Error ? e.message : String(e)}`));

      return;
    }

    if (contentParam) {
      try {
        const base64 = contentParam.replace(/-/g, '+').replace(/_/g, '/');
        const padded = base64 + '='.repeat((4 - (base64.length % 4)) % 4);
        const binary = atob(padded);
        const bytes = new Uint8Array(binary.length);
        for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
        const decoded = new TextDecoder().decode(bytes);
        onLoad({ content: decoded, filename: filenameParam, sourceUrl: null });
      } catch {
        onError('Failed to decode content. The embed URL may be malformed.');
      }
      return;
    }

    onError('No content provided. Add ?url= or ?content= to the URL.');
  }, [onLoad, onError]);

  return (
    <div className={styles.loadingState}>
      <div className={styles.loadingSpinner} />
      <span>Loading file…</span>
    </div>
  );
}

// ── Embed page inner ──────────────────────────────────────────────────────────

function EmbedInner() {
  const [file, setFile] = useState<LoadedFile | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loaded, setLoaded] = useState(false);

  const handleLoad = (f: LoadedFile) => { setFile(f); setLoaded(true); };
  const handleError = (msg: string) => { setError(msg); setLoaded(true); };

  return (
    <div className={styles.embedRoot}>
      {!loaded && <EmbedLoader onLoad={handleLoad} onError={handleError} />}

      {loaded && error && (
        <div className={styles.errorState}>
          <div className={styles.errorIcon}>⚠</div>
          <div className={styles.errorMsg}>{error}</div>
          <div className={styles.errorHint}>
            Example: <code>?url=https://github.com/owner/repo/blob/main/README.md</code>
          </div>
        </div>
      )}

      {loaded && file && (
        <Suspense fallback={<div className={styles.loadingState}><div className={styles.loadingSpinner} />Loading tool…</div>}>
          <AnnotatorTool compact initialFile={file} />
        </Suspense>
      )}

      <TreqBadge />
    </div>
  );
}

// ── Page ──────────────────────────────────────────────────────────────────────

export default function MarkdownAnnotatorEmbedPage() {
  return (
    <>
      <Head>
        <title>Markdown Annotator — Treq</title>
        <meta name="robots" content="noindex" />
        <style>{`
          body { margin: 0 !important; padding: 0 !important; overflow: hidden; }
          #__docusaurus { height: 100vh; overflow: hidden; }
        `}</style>
      </Head>
      <BrowserOnly fallback={<div style={{ padding: '2rem', textAlign: 'center' }}>Loading…</div>}>
        {() => <EmbedInner />}
      </BrowserOnly>
    </>
  );
}
