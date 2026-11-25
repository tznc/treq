import rough from 'roughjs';
import type { RoughSVG } from 'roughjs/bin/svg';

// ── Types ─────────────────────────────────────────────────────────────────────

export interface Commit {
  id: string;
  label: string;
  branch: string;
  parents: string[];
  isWorkingCopy?: boolean;
  bookmarks?: string[];
}

export interface Branch {
  name: string;
  color: string;
}

export interface GraphState {
  branches: Branch[];
  commits: Commit[];
  title: string;
}

export interface LayoutNode {
  commit: Commit;
  x: number;
  y: number;
  col: number;
  row: number;
}

export interface RenderOptions {
  highlightedId?: string | undefined;
  headLabel?: string | undefined;
}

// ── Default palette ───────────────────────────────────────────────────────────

export const DEFAULT_COLORS = [
  '#6366f1', // indigo
  '#f59e0b', // amber
  '#10b981', // emerald
  '#ef4444', // red
  '#8b5cf6', // violet
  '#06b6d4', // cyan
];

// ── Layout computation ────────────────────────────────────────────────────────

export function computeLayout(commits: Commit[], branches: Branch[]): LayoutNode[] {
  const HGAP = 160;
  const VGAP = 70;
  const MARGIN_X = 80;
  const MARGIN_Y = 60;

  const branchOrder = branches.map((b) => b.name);
  const colFor = (branchName: string) => {
    const i = branchOrder.indexOf(branchName);
    return i === -1 ? branchOrder.length : i;
  };

  const visited = new Set<string>();
  const sorted: Commit[] = [];
  const commitMap = new Map(commits.map((c) => [c.id, c]));

  function visit(id: string) {
    if (visited.has(id)) return;
    visited.add(id);
    const c = commitMap.get(id);
    if (c) {
      for (const p of c.parents) visit(p);
      sorted.push(c);
    }
  }
  for (const c of commits) visit(c.id);

  const rowOf = new Map<string, number>();
  for (const c of sorted) {
    const parentRows = c.parents.map((p) => rowOf.get(p) ?? -1);
    rowOf.set(c.id, parentRows.length ? Math.max(...parentRows) + 1 : 0);
  }

  return sorted.map((c) => {
    const col = colFor(c.branch);
    const row = rowOf.get(c.id) ?? 0;
    return {
      commit: c,
      col,
      row,
      x: MARGIN_X + col * HGAP,
      y: MARGIN_Y + row * VGAP,
    };
  });
}

// ── SVG rendering via Rough.js ─────────────────────────────────────────────

export function renderGraph(
  svgEl: SVGSVGElement,
  state: GraphState,
  dark: boolean,
  options?: RenderOptions,
) {
  while (svgEl.firstChild) svgEl.removeChild(svgEl.firstChild);

  const { commits, branches } = state;

  if (!commits.length) {
    svgEl.setAttribute('width', '280');
    svgEl.setAttribute('height', '80');
    svgEl.setAttribute('viewBox', '0 0 280 80');
    const msg = document.createElementNS('http://www.w3.org/2000/svg', 'text');
    msg.setAttribute('x', '140');
    msg.setAttribute('y', '44');
    msg.setAttribute('text-anchor', 'middle');
    msg.setAttribute('font-size', '13');
    msg.setAttribute('font-family', 'monospace');
    msg.setAttribute('fill', dark ? '#64748b' : '#94a3b8');
    msg.textContent = 'No commits yet';
    svgEl.appendChild(msg);
    return;
  }

  const nodes = computeLayout(commits, branches);
  const nodeMap = new Map(nodes.map((n) => [n.commit.id, n]));

  const colorOf = (branchName: string) => {
    const b = branches.find((br) => br.name === branchName);
    return b?.color ?? '#888';
  };

  const rc: RoughSVG = rough.svg(svgEl);
  const textColor = dark ? '#e2e8f0' : '#1e293b';
  const subtleColor = dark ? '#94a3b8' : '#64748b';
  const highlightedId = options?.highlightedId;

  const maxX = Math.max(...nodes.map((n) => n.x));
  const maxY = Math.max(...nodes.map((n) => n.y));
  const W = maxX + 220;
  const H = maxY + 100;
  svgEl.setAttribute('width', String(W));
  svgEl.setAttribute('height', String(H));
  svgEl.setAttribute('viewBox', `0 0 ${W} ${H}`);

  // Branch lane guidelines
  const branchCols = new Map<string, number>();
  for (const n of nodes) {
    if (!branchCols.has(n.commit.branch)) branchCols.set(n.commit.branch, n.x);
  }
  for (const [branchName, x] of branchCols) {
    const color = colorOf(branchName);
    const line = rc.line(x, 20, x, H - 20, {
      stroke: color,
      strokeWidth: 1,
      roughness: 0.8,
    });
    line.setAttribute('opacity', '0.15');
    svgEl.appendChild(line);

    const label = document.createElementNS('http://www.w3.org/2000/svg', 'text');
    label.setAttribute('x', String(x));
    label.setAttribute('y', '16');
    label.setAttribute('text-anchor', 'middle');
    label.setAttribute('font-size', '11');
    label.setAttribute('font-family', 'monospace');
    label.setAttribute('fill', color);
    label.setAttribute('opacity', '0.8');
    label.textContent = branchName;
    svgEl.appendChild(label);
  }

  // Edges
  for (const node of nodes) {
    for (const parentId of node.commit.parents) {
      const parent = nodeMap.get(parentId);
      if (!parent) continue;

      const color = colorOf(node.commit.branch);
      const isMerge = node.commit.parents.length > 1 && parentId !== node.commit.parents[0];

      if (node.x === parent.x) {
        svgEl.appendChild(
          rc.line(parent.x, parent.y, node.x, node.y, {
            stroke: isMerge ? colorOf(parent.commit.branch) : color,
            strokeWidth: 2,
            roughness: 1.2,
          }),
        );
      } else {
        const midY = (parent.y + node.y) / 2;
        svgEl.appendChild(
          rc.path(
            `M ${parent.x} ${parent.y} C ${parent.x} ${midY}, ${node.x} ${midY}, ${node.x} ${node.y}`,
            {
              stroke: isMerge ? colorOf(parent.commit.branch) : color,
              strokeWidth: 2,
              roughness: 1.2,
              fill: 'none',
            },
          ),
        );
      }
    }
  }

  // Commit nodes
  for (const node of nodes) {
    const { x, y, commit } = node;
    const color = colorOf(commit.branch);
    const isHighlighted = commit.id === highlightedId;
    const isWC = commit.isWorkingCopy;

    // Glow ring for highlighted commit
    if (isHighlighted) {
      const glow = rc.circle(x, y, 34, {
        stroke: color,
        strokeWidth: 1,
        fill: color,
        fillStyle: 'solid',
        roughness: 2,
      });
      glow.setAttribute('opacity', '0.18');
      svgEl.appendChild(glow);
    }

    // Main circle
    svgEl.appendChild(
      rc.circle(x, y, 22, {
        stroke: color,
        strokeWidth: isHighlighted ? 3 : 2,
        fill: dark ? '#1e293b' : '#ffffff',
        fillStyle: 'solid',
        roughness: isWC ? 0.4 : 1.5,
      }),
    );

    // Dashed overlay for working-copy commits
    if (isWC) {
      const dashed = document.createElementNS('http://www.w3.org/2000/svg', 'circle');
      dashed.setAttribute('cx', String(x));
      dashed.setAttribute('cy', String(y));
      dashed.setAttribute('r', '13');
      dashed.setAttribute('fill', 'none');
      dashed.setAttribute('stroke', color);
      dashed.setAttribute('stroke-width', '1.5');
      dashed.setAttribute('stroke-dasharray', '4 3');
      dashed.setAttribute('opacity', '0.6');
      svgEl.appendChild(dashed);
    }

    // Commit label
    const labelEl = document.createElementNS('http://www.w3.org/2000/svg', 'text');
    labelEl.setAttribute('x', String(x + 16));
    labelEl.setAttribute('y', String(y - 14));
    labelEl.setAttribute('font-size', '12');
    labelEl.setAttribute('font-family', 'sans-serif');
    labelEl.setAttribute('fill', textColor);
    labelEl.textContent = commit.label;
    svgEl.appendChild(labelEl);

    // Short ID inside circle
    const idEl = document.createElementNS('http://www.w3.org/2000/svg', 'text');
    idEl.setAttribute('x', String(x));
    idEl.setAttribute('y', String(y + 4));
    idEl.setAttribute('text-anchor', 'middle');
    idEl.setAttribute('font-size', '9');
    idEl.setAttribute('font-family', 'monospace');
    idEl.setAttribute('fill', subtleColor);
    idEl.textContent = commit.id.slice(0, 7);
    svgEl.appendChild(idEl);

    // HEAD / @ pointer to the left
    if (isHighlighted && options?.headLabel) {
      const ptr = document.createElementNS('http://www.w3.org/2000/svg', 'text');
      ptr.setAttribute('x', String(x - 16));
      ptr.setAttribute('y', String(y + 5));
      ptr.setAttribute('text-anchor', 'end');
      ptr.setAttribute('font-size', '11');
      ptr.setAttribute('font-family', 'monospace');
      ptr.setAttribute('font-weight', 'bold');
      ptr.setAttribute('fill', color);
      ptr.textContent = options.headLabel;
      svgEl.appendChild(ptr);
    }

    // Bookmark / ref pills below the commit
    if (commit.bookmarks && commit.bookmarks.length > 0) {
      commit.bookmarks.forEach((bm, bi) => {
        const by = y + 18 + bi * 15;
        const bx = x + 16;
        const pillW = Math.max(bm.length * 6.5 + 8, 30);

        const pill = document.createElementNS('http://www.w3.org/2000/svg', 'rect');
        pill.setAttribute('x', String(bx - 2));
        pill.setAttribute('y', String(by - 10));
        pill.setAttribute('width', String(pillW));
        pill.setAttribute('height', '13');
        pill.setAttribute('rx', '3');
        pill.setAttribute('fill', color);
        pill.setAttribute('opacity', '0.18');
        svgEl.appendChild(pill);

        const bmEl = document.createElementNS('http://www.w3.org/2000/svg', 'text');
        bmEl.setAttribute('x', String(bx + 2));
        bmEl.setAttribute('y', String(by));
        bmEl.setAttribute('font-size', '9.5');
        bmEl.setAttribute('font-family', 'monospace');
        bmEl.setAttribute('fill', color);
        bmEl.textContent = bm;
        svgEl.appendChild(bmEl);
      });
    }
  }
}
