import React, { useEffect, useRef, useState, useCallback } from 'react';
import Layout from '@theme/Layout';
import DownloadCTA from '@site/src/components/DownloadCTA';
import Head from '@docusaurus/Head';
import { useHistory, useLocation } from '@docusaurus/router';
import {
  type Commit,
  type Branch,
  type GraphState,
  DEFAULT_COLORS,
  renderGraph,
} from './_graph-shared';
import styles from './branch-visualizer.module.css';

// ── Serialisation helpers ─────────────────────────────────────────────────────

function encodeState(state: GraphState): string {
  return btoa(encodeURIComponent(JSON.stringify(state)));
}

function decodeState(raw: string): GraphState | null {
  try {
    return JSON.parse(decodeURIComponent(atob(raw)));
  } catch {
    return null;
  }
}

// ── Default graph ────────────────────────────────────────────────────────────

const DEFAULT_STATE: GraphState = {
  title: 'My Branch Diagram',
  branches: [
    { name: 'main', color: '#6366f1' },
    { name: 'feature/auth', color: '#f59e0b' },
    { name: 'feature/ui', color: '#10b981' },
  ],
  commits: [
    { id: 'c1', label: 'Initial commit', branch: 'main', parents: [] },
    { id: 'c2', label: 'Add routing', branch: 'main', parents: ['c1'] },
    { id: 'c3', label: 'Start auth', branch: 'feature/auth', parents: ['c2'] },
    { id: 'c4', label: 'Fix typo', branch: 'main', parents: ['c2'] },
    { id: 'c5', label: 'Add login', branch: 'feature/auth', parents: ['c3'] },
    { id: 'c6', label: 'Start UI', branch: 'feature/ui', parents: ['c4'] },
    { id: 'c7', label: 'Merge auth', branch: 'main', parents: ['c4', 'c5'] },
    { id: 'c8', label: 'Add styles', branch: 'feature/ui', parents: ['c6'] },
    { id: 'c9', label: 'Merge UI', branch: 'main', parents: ['c7', 'c8'] },
  ],
};

// ── Editor sub-components ─────────────────────────────────────────────────────

function BranchEditor({
  branches,
  onChange,
}: {
  branches: Branch[];
  onChange: (b: Branch[]) => void;
}) {
  const add = () =>
    onChange([
      ...branches,
      { name: `branch-${branches.length + 1}`, color: DEFAULT_COLORS[branches.length % DEFAULT_COLORS.length] ?? '#888' },
    ]);

  const remove = (i: number) => onChange(branches.filter((_, idx) => idx !== i));

  const update = (i: number, field: keyof Branch, val: string) =>
    onChange(branches.map((b, idx) => (idx === i ? { ...b, [field]: val } : b)));

  return (
    <div className={styles.section}>
      <div className={styles.sectionHeader}>
        <span className={styles.sectionTitle}>Branches</span>
        <button className={styles.addBtn} onClick={add}>+ Add</button>
      </div>
      {branches.map((b, i) => (
        <div key={i} className={styles.row}>
          <input
            className={styles.input}
            value={b.name}
            onChange={(e) => update(i, 'name', e.target.value)}
            placeholder="branch name"
          />
          <input
            type="color"
            className={styles.colorPicker}
            value={b.color}
            onChange={(e) => update(i, 'color', e.target.value)}
          />
          <button className={styles.removeBtn} onClick={() => remove(i)}>✕</button>
        </div>
      ))}
    </div>
  );
}

function CommitEditor({
  commits,
  branches,
  onChange,
}: {
  commits: Commit[];
  branches: Branch[];
  onChange: (c: Commit[]) => void;
}) {
  const add = () =>
    onChange([
      ...commits,
      { id: `c${commits.length + 1}`, label: 'New commit', branch: branches[0]?.name ?? 'main', parents: [] },
    ]);

  const remove = (i: number) => onChange(commits.filter((_, idx) => idx !== i));

  const update = (i: number, field: keyof Commit, val: string | string[]) =>
    onChange(commits.map((c, idx) => (idx === i ? { ...c, [field]: val } : c)));

  const toggleParent = (i: number, parentId: string) => {
    const commit = commits[i];
    if (!commit) return;
    const current = commit.parents;
    const next = current.includes(parentId)
      ? current.filter((p) => p !== parentId)
      : [...current, parentId];
    update(i, 'parents', next);
  };

  return (
    <div className={styles.section}>
      <div className={styles.sectionHeader}>
        <span className={styles.sectionTitle}>Commits</span>
        <button className={styles.addBtn} onClick={add}>+ Add</button>
      </div>
      {commits.map((c, i) => (
        <div key={i} className={styles.commitBlock}>
          <div className={styles.row}>
            <input
              className={styles.input}
              style={{ width: '80px' }}
              value={c.id}
              onChange={(e) => update(i, 'id', e.target.value)}
              placeholder="id"
            />
            <input
              className={styles.input}
              value={c.label}
              onChange={(e) => update(i, 'label', e.target.value)}
              placeholder="label"
            />
            <select
              className={styles.select}
              value={c.branch}
              onChange={(e) => update(i, 'branch', e.target.value)}
            >
              {branches.map((b) => (
                <option key={b.name} value={b.name}>{b.name}</option>
              ))}
            </select>
            <button className={styles.removeBtn} onClick={() => remove(i)}>✕</button>
          </div>
          <div className={styles.parentRow}>
            <span className={styles.parentLabel}>Parents:</span>
            {commits.filter((_, pi) => pi < i).map((p) => (
              <label key={p.id} className={styles.parentCheck}>
                <input
                  type="checkbox"
                  checked={c.parents.includes(p.id)}
                  onChange={() => toggleParent(i, p.id)}
                />
                {p.id}
              </label>
            ))}
          </div>
        </div>
      ))}
    </div>
  );
}

// ── Toast ─────────────────────────────────────────────────────────────────────

function useToast() {
  const [msg, setMsg] = useState<string | null>(null);
  const show = useCallback((m: string) => {
    setMsg(m);
    setTimeout(() => setMsg(null), 2500);
  }, []);
  return { msg, show };
}

// ── Main page ─────────────────────────────────────────────────────────────────

const SCHEMA = {
  '@context': 'https://schema.org',
  '@type': 'WebApplication',
  name: 'Branch Visualizer',
  description:
    'Sketch git branch diagrams with a hand-drawn aesthetic. Shareable via URL.',
  url: 'https://treq.dev/tools/branch-visualizer',
  applicationCategory: 'DeveloperApplication',
  operatingSystem: 'Any',
  isAccessibleForFree: true,
  provider: {
    '@type': 'Organization',
    name: 'Treq',
    url: 'https://treq.dev',
  },
};

export default function BranchVisualizerPage() {
  const location = useLocation();
  const history = useHistory();
  const svgRef = useRef<SVGSVGElement>(null);
  const { msg: toast, show: showToast } = useToast();

  // Load state from URL or use default
  const [graphState, setGraphState] = useState<GraphState>(() => {
    const params = new URLSearchParams(location.search);
    const raw = params.get('s');
    if (raw) {
      const decoded = decodeState(raw);
      if (decoded) return decoded;
    }
    return DEFAULT_STATE;
  });

  const [dark, setDark] = useState(false);

  // Detect colour scheme
  useEffect(() => {
    const mq = window.matchMedia('(prefers-color-scheme: dark)');
    setDark(mq.matches);
    const handler = (e: MediaQueryListEvent) => setDark(e.matches);
    mq.addEventListener('change', handler);
    return () => mq.removeEventListener('change', handler);
  }, []);

  // Sync state → URL
  useEffect(() => {
    const params = new URLSearchParams();
    params.set('s', encodeState(graphState));
    history.replace({ search: params.toString() });
  }, [graphState]);

  // Re-render graph
  useEffect(() => {
    if (!svgRef.current) return;
    renderGraph(svgRef.current, graphState, dark);
  }, [graphState, dark]);

  // ── Export helpers ──────────────────────────────────────────────────────────

  const copySvg = () => {
    if (!svgRef.current) return;
    const serializer = new XMLSerializer();
    const svgStr = serializer.serializeToString(svgRef.current);
    navigator.clipboard.writeText(svgStr).then(() => showToast('SVG copied to clipboard!'));
  };

  const copyPng = () => {
    if (!svgRef.current) return;
    const serializer = new XMLSerializer();
    const svgStr = serializer.serializeToString(svgRef.current);
    const blob = new Blob([svgStr], { type: 'image/svg+xml' });
    const url = URL.createObjectURL(blob);
    const img = new Image();
    img.onload = () => {
      const scale = 2;
      const canvas = document.createElement('canvas');
      canvas.width = img.width * scale;
      canvas.height = img.height * scale;
      const ctx = canvas.getContext('2d')!;
      ctx.scale(scale, scale);
      ctx.fillStyle = dark ? '#0f172a' : '#ffffff';
      ctx.fillRect(0, 0, img.width, img.height);
      ctx.drawImage(img, 0, 0);
      canvas.toBlob((pngBlob) => {
        if (!pngBlob) return;
        navigator.clipboard.write([new ClipboardItem({ 'image/png': pngBlob })]).then(() =>
          showToast('PNG copied to clipboard!')
        );
      }, 'image/png');
      URL.revokeObjectURL(url);
    };
    img.src = url;
  };

  const copyLink = () => {
    navigator.clipboard.writeText(window.location.href).then(() =>
      showToast('Link copied to clipboard!')
    );
  };

  const downloadSvg = () => {
    if (!svgRef.current) return;
    const serializer = new XMLSerializer();
    const svgStr = serializer.serializeToString(svgRef.current);
    const blob = new Blob([svgStr], { type: 'image/svg+xml' });
    const a = document.createElement('a');
    a.href = URL.createObjectURL(blob);
    a.download = `${graphState.title.replace(/\s+/g, '-').toLowerCase()}.svg`;
    a.click();
  };

  // ── Render ──────────────────────────────────────────────────────────────────

  return (
    <Layout
      title="Branch Visualizer"
      description="Visualize git branch structures with a hand-drawn aesthetic. Shareable via URL."
    >
      <Head>
        <script type="application/ld+json">{JSON.stringify(SCHEMA)}</script>
      </Head>
      <div className={styles.page}>
        {toast && <div className={styles.toast}>{toast}</div>}

        <div className={styles.header}>
          <div className={styles.breadcrumb}>
            <a href="/tools">Tools</a>
            <span> / </span>
            <span>Branch Visualizer</span>
          </div>
          <h1 className={styles.pageTitle}>Branch Visualizer</h1>
          <p className={styles.pageSubtitle}>
            Sketch git branch diagrams with a hand-drawn look. Configuration is saved in the URL — just share the link.
          </p>
        </div>

        <div className={styles.workspace}>
          {/* Left panel: editor */}
          <div className={styles.editor}>
            <div className={styles.section}>
              <label className={styles.sectionTitle}>Diagram title</label>
              <input
                className={styles.input}
                style={{ marginTop: '6px' }}
                value={graphState.title}
                onChange={(e) => setGraphState((s) => ({ ...s, title: e.target.value }))}
              />
            </div>

            <BranchEditor
              branches={graphState.branches}
              onChange={(branches) => setGraphState((s) => ({ ...s, branches }))}
            />

            <CommitEditor
              commits={graphState.commits}
              branches={graphState.branches}
              onChange={(commits) => setGraphState((s) => ({ ...s, commits }))}
            />
          </div>

          {/* Right panel: preview + actions */}
          <div className={styles.preview}>
            <div className={styles.previewHeader}>
              <span className={styles.previewTitle}>{graphState.title}</span>
              <div className={styles.actions}>
                <button className={styles.actionBtn} onClick={copyLink} title="Copy shareable link">
                  🔗 Copy Link
                </button>
                <button className={styles.actionBtn} onClick={copySvg} title="Copy SVG to clipboard">
                  SVG
                </button>
                <button className={styles.actionBtn} onClick={copyPng} title="Copy PNG to clipboard">
                  PNG
                </button>
                <button className={styles.actionBtn} onClick={downloadSvg} title="Download SVG">
                  ↓ Download
                </button>
              </div>
            </div>
            <div className={styles.canvas}>
              <svg ref={svgRef} />
            </div>
          </div>
        </div>
      </div>
      <DownloadCTA title="Built by Treq" />
    </Layout>
  );
}
