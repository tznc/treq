import type React from 'react';
import {
  useState,
  useCallback,
  useEffect,
  useMemo,
  useRef,type 
  KeyboardEvent,
} from 'react'
import {
  ReactFlow,
  ReactFlowProvider,
  Background,
  BackgroundVariant,
  Controls,
  MiniMap,
  Panel,
  Handle,
  Position,
  addEdge,
  useNodesState,
  useEdgesState,
  useReactFlow,
  MarkerType,
  NodeResizer,
} from '@xyflow/react';
import type {
  Node,
  Edge,
  Connection,
  OnConnect,
  NodeProps,
} from '@xyflow/react';
import '@xyflow/react/dist/style.css';
import { useLocation, useHistory } from '@docusaurus/router';
import dagre from '@dagrejs/dagre';
import styles from './dag-visualizer.module.css';

// ── Types ─────────────────────────────────────────────────────────────────────

export type WorkflowNodeData = {
  name: string;
  prompt: string;
  skills: string[];
  agent?: string;
  model?: string;
};

export type GroupNodeData = {
  name: string;
};

type WorkflowNode = Node<WorkflowNodeData, 'workflow'>;
type GroupNode = Node<GroupNodeData, 'group'>;

interface SerializedNode {
  id: string;
  type: 'workflow' | 'group';
  position: { x: number; y: number };
  data: WorkflowNodeData | GroupNodeData;
  parentId?: string;
  width?: number;
  height?: number;
}

interface SerializedEdge {
  id: string;
  source: string;
  target: string;
  delegation?: boolean;
}

interface SerializedState {
  nodes: SerializedNode[];
  edges: SerializedEdge[];
}

// ── Constants ─────────────────────────────────────────────────────────────────

const SKILL_SUGGESTIONS = [
  '/code-review',
  '/run-tests',
  '/lint',
  '/deploy',
  '/summarize',
  '/generate-docs',
  '/security-review',
  '/optimize',
  '/refactor',
  '/document',
  '/analyze',
  '/plan',
  '/branch-visualizer',
];

const AVAILABLE_AGENTS = ['claude', 'codex', 'cursor'];

const NODE_W = 240;
const NODE_H = 130;
const GROUP_PAD = 40;

// ── Edge defaults ─────────────────────────────────────────────────────────────

const EDGE_DEFAULTS = {
  type: 'smoothstep' as const,
  animated: true,
  markerEnd: { type: MarkerType.ArrowClosed, color: '#6366f1' },
  style: { stroke: '#6366f1', strokeWidth: 2 },
};

const DELEGATION_EDGE_DEFAULTS = {
  type: 'smoothstep' as const,
  animated: false,
  markerEnd: { type: MarkerType.ArrowClosed, color: '#f59e0b' },
  style: { stroke: '#f59e0b', strokeWidth: 2.5, strokeDasharray: '8 4' },
};

// ── URL serialisation ─────────────────────────────────────────────────────────

function encodeState(nodes: Node[], edges: Edge[]): string {
  const state: SerializedState = {
    nodes: nodes.map((n) => {
      const sn: SerializedNode = {
        id: n.id,
        type: (n.type ?? 'workflow') as 'workflow' | 'group',
        position: { x: Math.round(n.position.x), y: Math.round(n.position.y) },
        data: n.data as WorkflowNodeData | GroupNodeData,
      };
      if (n.parentId) sn.parentId = n.parentId;
      if (n.type === 'group') {
        const sw = n.style?.width;
        const sh = n.style?.height;
        sn.width = typeof sw === 'number' ? sw : 400;
        sn.height = typeof sh === 'number' ? sh : 300;
      }
      return sn;
    }),
    edges: edges.map((e) => {
      const se: SerializedEdge = { id: e.id, source: e.source, target: e.target };
      if (e.data?.delegation) se.delegation = true;
      return se;
    }),
  };
  return btoa(encodeURIComponent(JSON.stringify(state)));
}

function decodeState(raw: string): SerializedState | null {
  try {
    return JSON.parse(decodeURIComponent(atob(raw)));
  } catch {
    return null;
  }
}

function stateToNodes(s: SerializedState): Node[] {
  return s.nodes.map((n) => {
    const base = {
      id: n.id,
      position: n.position,
      data: n.data,
      ...(n.parentId ? { parentId: n.parentId } : {}),
    };
    if (n.type === 'group') {
      return {
        ...base,
        type: 'group' as const,
        style: { width: n.width ?? 400, height: n.height ?? 300 },
      };
    }
    return { ...base, type: 'workflow' as const };
  });
}

function stateToEdges(s: SerializedState): Edge[] {
  return s.edges.map((e) => ({
    ...e,
    ...(e.delegation ? DELEGATION_EDGE_DEFAULTS : EDGE_DEFAULTS),
    data: { delegation: e.delegation ?? false },
  }));
}

// ── Dagre auto-layout ─────────────────────────────────────────────────────────

function applyDagreLayout(nodes: Node[], edges: Edge[]): Node[] {
  const g = new dagre.graphlib.Graph();
  g.setDefaultEdgeLabel(() => ({}));
  g.setGraph({ rankdir: 'LR', ranksep: 80, nodesep: 40 });

  const topLevel = nodes.filter((n) => !n.parentId);

  topLevel.forEach((n) => {
    const sw = n.style?.width;
    const sh = n.style?.height;
    const w = n.type === 'group' ? (typeof sw === 'number' ? sw : 400) : NODE_W;
    const h = n.type === 'group' ? (typeof sh === 'number' ? sh : 300) : NODE_H;
    g.setNode(n.id, { width: w, height: h });
  });

  const topIds = new Set(topLevel.map((n) => n.id));
  edges.forEach((e) => {
    if (topIds.has(e.source) && topIds.has(e.target)) g.setEdge(e.source, e.target);
  });

  dagre.layout(g);

  return nodes.map((n) => {
    if (n.parentId) return n;
    const ln = g.node(n.id);
    if (!ln) return n;
    const sw = n.style?.width;
    const sh = n.style?.height;
    const w = n.type === 'group' ? (typeof sw === 'number' ? sw : 400) : NODE_W;
    const h = n.type === 'group' ? (typeof sh === 'number' ? sh : 300) : NODE_H;
    return { ...n, position: { x: ln.x - w / 2, y: ln.y - h / 2 } };
  });
}

// ── Default state ─────────────────────────────────────────────────────────────

const DEFAULT_NODES: Node[] = [
  {
    id: 'n1',
    type: 'workflow',
    position: { x: 0, y: 80 },
    data: {
      name: 'Requirements',
      prompt: 'Analyze the user story and acceptance criteria. Identify ambiguities and list open questions.',
      skills: ['/analyze', '/summarize'],
      agent: 'claude',
      model: 'opus',
    } as WorkflowNodeData,
  },
  {
    id: 'n2',
    type: 'workflow',
    position: { x: 320, y: 80 },
    data: {
      name: 'Architecture',
      prompt: 'Design the system architecture. Choose appropriate patterns and data models.',
      skills: ['/plan', '/document'],
      agent: 'claude',
    } as WorkflowNodeData,
  },
  {
    id: 'n3',
    type: 'workflow',
    position: { x: 640, y: 20 },
    data: {
      name: 'Implementation',
      prompt: 'Write the feature code following the architecture spec. Keep functions small and testable.',
      skills: ['/lint', '/optimize'],
    } as WorkflowNodeData,
  },
  {
    id: 'n4',
    type: 'workflow',
    position: { x: 640, y: 160 },
    data: {
      name: 'Tests',
      prompt: 'Write unit and integration tests covering happy paths and edge cases.',
      skills: ['/run-tests', '/generate-docs'],
    } as WorkflowNodeData,
  },
  {
    id: 'n5',
    type: 'workflow',
    position: { x: 960, y: 80 },
    data: {
      name: 'Code Review',
      prompt: 'Review the diff for correctness, security issues, and style violations.',
      skills: ['/code-review', '/security-review'],
      agent: 'claude',
      model: 'sonnet',
    } as WorkflowNodeData,
  },
  {
    id: 'n6',
    type: 'workflow',
    position: { x: 1280, y: 80 },
    data: {
      name: 'Deploy',
      prompt: 'Deploy to staging, run smoke tests, then promote to production.',
      skills: ['/deploy', '/run-tests'],
    } as WorkflowNodeData,
  },
];

const DEFAULT_EDGES: Edge[] = [
  { id: 'e1-2', source: 'n1', target: 'n2', ...EDGE_DEFAULTS, data: { delegation: false } },
  { id: 'e2-3', source: 'n2', target: 'n3', ...EDGE_DEFAULTS, data: { delegation: false } },
  { id: 'e2-4', source: 'n2', target: 'n4', ...DELEGATION_EDGE_DEFAULTS, data: { delegation: true } },
  { id: 'e3-5', source: 'n3', target: 'n5', ...EDGE_DEFAULTS, data: { delegation: false } },
  { id: 'e4-5', source: 'n4', target: 'n5', ...EDGE_DEFAULTS, data: { delegation: false } },
  { id: 'e5-6', source: 'n5', target: 'n6', ...EDGE_DEFAULTS, data: { delegation: false } },
];

// ── WorkflowNode component ────────────────────────────────────────────────────

function WorkflowNode({ data, selected }: NodeProps<WorkflowNode>) {
  return (
    <div className={`${styles.node} ${selected ? styles.nodeSelected : ''}`} data-testid="flow-node">
      <Handle type="target" position={Position.Left} className={styles.handle} />

      <div className={styles.nodeHeader}>
        <span className={styles.nodeName}>{data.name}</span>
        {(data.agent || data.model) && (
          <div className={styles.nodeMeta}>
            {data.agent && <span className={styles.nodeAgent}>{data.agent}</span>}
            {data.model && <span className={styles.nodeModel}>{data.model}</span>}
          </div>
        )}
      </div>

      {data.prompt && (
        <div className={styles.nodePrompt}>
          {data.prompt.length > 80 ? data.prompt.slice(0, 77) + '…' : data.prompt}
        </div>
      )}

      {data.skills.length > 0 && (
        <div className={styles.nodeSkills}>
          {data.skills.map((s) => (
            <span key={s} className={styles.skill}>{s}</span>
          ))}
        </div>
      )}

      <Handle type="source" position={Position.Right} className={styles.handle} />
    </div>
  );
}

// ── GroupNode component ───────────────────────────────────────────────────────

function GroupNode({ data, selected }: NodeProps<GroupNode>) {
  return (
    <div className={`${styles.groupNode} ${selected ? styles.groupNodeSelected : ''}`}>
      <NodeResizer isVisible={selected} minWidth={160} minHeight={120} lineStyle={{ stroke: '#6366f1' }} handleStyle={{ background: '#6366f1', border: '2px solid #fff' }} />
      <Handle type="target" position={Position.Left} className={styles.groupHandle} />
      <div className={styles.groupLabel}>{data.name || 'Group'}</div>
      <Handle type="source" position={Position.Right} className={styles.groupHandle} />
    </div>
  );
}

const nodeTypes = { workflow: WorkflowNode, group: GroupNode };

// ── Skill input with autocomplete ─────────────────────────────────────────────

function SkillInput({ onAdd }: { onAdd: (skill: string) => void }) {
  const [val, setVal] = useState('');
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  const filtered = val
    ? SKILL_SUGGESTIONS.filter(
        (s) => s.includes(val.toLowerCase()) || val.toLowerCase().includes(s.slice(1, 4)),
      )
    : SKILL_SUGGESTIONS;

  const commit = (s: string) => {
    const trimmed = s.trim();
    if (!trimmed) return;
    onAdd(trimmed.startsWith('/') ? trimmed : `/${trimmed}`);
    setVal('');
    setOpen(false);
  };

  const onKey = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Enter') commit(val);
    if (e.key === 'Escape') setOpen(false);
  };

  useEffect(() => {
    const handler = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as HTMLElement)) setOpen(false);
    };
    document.addEventListener('mousedown', handler);
    return () => document.removeEventListener('mousedown', handler);
  }, []);

  return (
    <div className={styles.skillInputWrap} ref={ref}>
      <div className={styles.skillInputRow}>
        <input
          className={styles.skillInput}
          value={val}
          placeholder="/skill-name"
          onChange={(e) => { setVal(e.target.value); setOpen(true); }}
          onFocus={() => setOpen(true)}
          onKeyDown={onKey}
        />
        <button className={styles.addSkillBtn} onClick={() => commit(val)}>Add</button>
      </div>
      {open && filtered.length > 0 && (
        <div className={styles.skillDropdown}>
          {filtered.map((s) => (
            <div key={s} className={styles.skillOption} onMouseDown={() => commit(s)}>
              {s}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

// ── Node detail panel ─────────────────────────────────────────────────────────

interface NodePanelProps {
  node: Node<WorkflowNodeData>;
  onChange: (id: string, patch: Partial<WorkflowNodeData>) => void;
  onDelete: (id: string) => void;
  onClose: () => void;
}

function NodeDetailPanel({ node, onChange, onDelete, onClose }: NodePanelProps) {
  const { data, id } = node;

  return (
    <div className={styles.detailPanel}>
      <div className={styles.panelHeader}>
        <span className={styles.panelTitle}>Edit Node</span>
        <button className={styles.panelClose} onClick={onClose} aria-label="Close panel">✕</button>
      </div>

      <div className={styles.panelBody}>
        <label className={styles.fieldLabel}>Name</label>
        <input
          className={styles.fieldInput}
          value={data.name}
          onChange={(e) => onChange(id, { name: e.target.value })}
        />

        <label className={styles.fieldLabel}>Agent</label>
        <select
          className={styles.fieldSelect}
          value={data.agent ?? ''}
          onChange={(e) => onChange(id, { agent: e.target.value || undefined })}
        >
          <option value="">— inherit default —</option>
          {AVAILABLE_AGENTS.map((a) => (
            <option key={a} value={a}>{a}</option>
          ))}
        </select>

        <label className={styles.fieldLabel}>Model</label>
        <select
          className={styles.fieldSelect}
          value={data.model ?? ''}
          onChange={(e) => onChange(id, { model: e.target.value || undefined })}
        >
          <option value="">— inherit default —</option>
          <option value="sonnet">sonnet</option>
          <option value="opus">opus</option>
          <option value="haiku">haiku</option>
          <option value="sonnet[1m]">sonnet (1M ctx)</option>
          <option value="opusplan">opus plan</option>
        </select>

        <label className={styles.fieldLabel}>Prompt</label>
        <textarea
          className={styles.fieldTextarea}
          rows={6}
          value={data.prompt}
          onChange={(e) => onChange(id, { prompt: e.target.value })}
          placeholder="Describe what the AI agent should do at this step…"
        />

        <label className={styles.fieldLabel}>Slash Skills</label>
        <div className={styles.skillTags}>
          {data.skills.map((s) => (
            <span key={s} className={styles.skillTag}>
              {s}
              <button
                className={styles.skillRemove}
                onClick={() => onChange(id, { skills: data.skills.filter((x) => x !== s) })}
                aria-label={`Remove ${s}`}
              >
                ✕
              </button>
            </span>
          ))}
        </div>
        <SkillInput
          onAdd={(skill) => {
            if (!data.skills.includes(skill)) onChange(id, { skills: [...data.skills, skill] });
          }}
        />
      </div>

      <div className={styles.panelFooter}>
        <button className={styles.deleteBtn} onClick={() => onDelete(id)}>
          Delete node
        </button>
      </div>
    </div>
  );
}

// ── Group detail panel ────────────────────────────────────────────────────────

interface GroupPanelProps {
  node: Node<GroupNodeData>;
  onChange: (id: string, name: string) => void;
  onDelete: (id: string) => void;
  onClose: () => void;
}

function GroupDetailPanel({ node, onChange, onDelete, onClose }: GroupPanelProps) {
  return (
    <div className={styles.detailPanel}>
      <div className={styles.panelHeader}>
        <span className={styles.panelTitle}>Edit Group</span>
        <button className={styles.panelClose} onClick={onClose} aria-label="Close panel">✕</button>
      </div>
      <div className={styles.panelBody}>
        <label className={styles.fieldLabel}>Label</label>
        <input
          className={styles.fieldInput}
          value={node.data.name}
          placeholder="Group name…"
          onChange={(e) => onChange(node.id, e.target.value)}
        />
        <p className={styles.fieldHint}>
          Drag nodes inside the dotted box to associate them. Resize the box by dragging its edges when selected.
          Connect arrows to the group's left/right handles to route flows through the whole group.
        </p>
      </div>
      <div className={styles.panelFooter}>
        <button className={styles.deleteBtn} onClick={() => onDelete(node.id)}>
          Ungroup &amp; delete box
        </button>
      </div>
    </div>
  );
}

// ── Edge detail panel ─────────────────────────────────────────────────────────

interface EdgePanelProps {
  edge: Edge;
  onToggleDelegation: (id: string) => void;
  onDelete: (id: string) => void;
  onClose: () => void;
}

function EdgeDetailPanel({ edge, onToggleDelegation, onDelete, onClose }: EdgePanelProps) {
  const isDelegation = !!(edge.data?.delegation);

  return (
    <div className={styles.detailPanel}>
      <div className={styles.panelHeader}>
        <span className={styles.panelTitle}>Edit Connection</span>
        <button className={styles.panelClose} onClick={onClose} aria-label="Close panel">✕</button>
      </div>
      <div className={styles.panelBody}>
        <label className={styles.fieldLabel}>Flow type</label>
        <div className={styles.toggleRow}>
          <button
            className={`${styles.toggleBtn} ${!isDelegation ? styles.toggleBtnActive : ''}`}
            onClick={() => { if (isDelegation) onToggleDelegation(edge.id); }}
          >
            Standard
          </button>
          <button
            className={`${styles.toggleBtn} ${isDelegation ? styles.toggleBtnDelegate : ''}`}
            onClick={() => { if (!isDelegation) onToggleDelegation(edge.id); }}
          >
            Delegation
          </button>
        </div>
        <p className={styles.fieldHint}>
          {isDelegation
            ? 'Delegation — this agent sub-tasks to another agent and awaits the result.'
            : 'Standard — output from one step is passed as input to the next.'}
        </p>

        <div className={styles.edgeLegend}>
          <div className={styles.edgeLegendRow}>
            <svg width="40" height="12"><line x1="0" y1="6" x2="40" y2="6" stroke="#6366f1" strokeWidth="2" /></svg>
            <span>Standard flow</span>
          </div>
          <div className={styles.edgeLegendRow}>
            <svg width="40" height="12"><line x1="0" y1="6" x2="40" y2="6" stroke="#f59e0b" strokeWidth="2.5" strokeDasharray="8 4" /></svg>
            <span>Delegation flow</span>
          </div>
        </div>
      </div>
      <div className={styles.panelFooter}>
        <button className={styles.deleteBtn} onClick={() => onDelete(edge.id)}>
          Delete connection
        </button>
      </div>
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

// ── Canvas (needs to be inside ReactFlowProvider) ─────────────────────────────

function DagCanvas() {
  const location = useLocation();
  const history = useHistory();
  const { fitView } = useReactFlow();
  const { msg: toast, show: showToast } = useToast();

  const initialNodes = useMemo<Node[]>(() => {
    const params = new URLSearchParams(location.search);
    const raw = params.get('s');
    if (raw) { const s = decodeState(raw); if (s) return stateToNodes(s); }
    return DEFAULT_NODES;
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const initialEdges = useMemo<Edge[]>(() => {
    const params = new URLSearchParams(location.search);
    const raw = params.get('s');
    if (raw) { const s = decodeState(raw); if (s) return stateToEdges(s); }
    return DEFAULT_EDGES;
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const [nodes, setNodes, onNodesChange] = useNodesState(initialNodes);
  const [edges, setEdges, onEdgesChange] = useEdgesState(initialEdges);

  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [selectedEdgeId, setSelectedEdgeId] = useState<string | null>(null);

  const selectedNode = selectedId ? nodes.find((n) => n.id === selectedId) : undefined;
  const selectedEdge = !selectedId && selectedEdgeId ? edges.find((e) => e.id === selectedEdgeId) : undefined;

  // Clean up selectedEdgeId if edge was deleted via Delete key
  useEffect(() => {
    if (selectedEdgeId && !edges.find((e) => e.id === selectedEdgeId)) {
      setSelectedEdgeId(null);
    }
  }, [edges, selectedEdgeId]);

  // URL sync (debounced)
  const syncTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  useEffect(() => {
    if (syncTimer.current) clearTimeout(syncTimer.current);
    syncTimer.current = setTimeout(() => {
      const params = new URLSearchParams();
      params.set('s', encodeState(nodes, edges));
      history.replace({ search: params.toString() });
    }, 600);
    return () => { if (syncTimer.current) clearTimeout(syncTimer.current); };
  }, [nodes, edges]);

  // Connect — new edges default to standard flow
  const onConnect: OnConnect = useCallback(
    (conn: Connection) =>
      setEdges((eds) =>
        addEdge(
          { ...conn, ...EDGE_DEFAULTS, id: `e${conn.source}-${conn.target}-${Date.now()}`, data: { delegation: false } },
          eds,
        ),
      ),
    [setEdges],
  );

  // Add workflow node
  const addNode = useCallback(() => {
    const maxX = nodes.reduce((m, n) => Math.max(m, n.position.x), 0);
    const id = `n${Date.now()}`;
    const newNode: WorkflowNode = {
      id,
      type: 'workflow',
      position: { x: maxX + 320, y: 80 },
      data: { name: 'New Step', prompt: '', skills: [] },
    };
    setNodes((nds) => [...nds, newNode]);
    setSelectedId(id);
    setSelectedEdgeId(null);
  }, [nodes, setNodes]);

  // Group selected nodes (those with node.selected = true via shift-click or drag-select)
  const groupSelected = useCallback(() => {
    const sel = nodes.filter((n) => n.selected && !n.parentId && n.type !== 'group');
    if (sel.length === 0) {
      showToast('Shift-click or drag-select nodes first');
      return;
    }
    const minX = Math.min(...sel.map((n) => n.position.x)) - GROUP_PAD;
    const minY = Math.min(...sel.map((n) => n.position.y)) - GROUP_PAD;
    const maxX = Math.max(...sel.map((n) => n.position.x + NODE_W)) + GROUP_PAD;
    const maxY = Math.max(...sel.map((n) => n.position.y + NODE_H)) + GROUP_PAD;

    const gid = `g${Date.now()}`;
    const groupNode: GroupNode = {
      id: gid,
      type: 'group',
      position: { x: minX, y: minY },
      style: { width: maxX - minX, height: maxY - minY },
      data: { name: 'Group' },
    };

    const selIds = new Set(sel.map((n) => n.id));
    setNodes((nds) => {
      const updated = nds.map((n) =>
        selIds.has(n.id)
          ? {
              ...n,
              position: { x: n.position.x - minX, y: n.position.y - minY },
              parentId: gid,
              selected: false,
            }
          : n,
      );
      const nonChildren = updated.filter((n) => n.parentId !== gid);
      const children = updated.filter((n) => n.parentId === gid);
      return [...nonChildren, groupNode, ...children];
    });
    setSelectedId(gid);
    setSelectedEdgeId(null);
  }, [nodes, setNodes, showToast]);

  // Update workflow node data
  const updateNode = useCallback((id: string, patch: Partial<WorkflowNodeData>) => {
    setNodes((nds) =>
      nds.map((n) => n.id === id ? { ...n, data: { ...n.data as WorkflowNodeData, ...patch } } : n),
    );
  }, [setNodes]);

  // Update group label
  const updateGroup = useCallback((id: string, name: string) => {
    setNodes((nds) =>
      nds.map((n) => n.id === id ? { ...n, data: { name } } : n),
    );
  }, [setNodes]);

  // Delete node — for groups, unparent children and restore absolute positions
  const deleteNode = useCallback((id: string) => {
    setNodes((nds) => {
      const target = nds.find((n) => n.id === id);
      if (target?.type === 'group') {
        return nds
          .filter((n) => n.id !== id)
          .map((n) => {
            if (n.parentId !== id) return n;
            return {
              ...n,
              parentId: undefined,
              position: {
                x: n.position.x + target.position.x,
                y: n.position.y + target.position.y,
              },
            };
          });
      }
      return nds.filter((n) => n.id !== id);
    });
    setEdges((eds) => eds.filter((e) => e.source !== id && e.target !== id));
    setSelectedId(null);
  }, [setNodes, setEdges]);

  // Toggle delegation on an edge
  const toggleEdgeDelegation = useCallback((id: string) => {
    setEdges((eds) =>
      eds.map((e) => {
        if (e.id !== id) return e;
        const next = !e.data?.delegation;
        return {
          ...e,
          ...(next ? DELEGATION_EDGE_DEFAULTS : EDGE_DEFAULTS),
          data: { delegation: next },
        };
      }),
    );
  }, [setEdges]);

  // Delete edge
  const deleteEdge = useCallback((id: string) => {
    setEdges((eds) => eds.filter((e) => e.id !== id));
    setSelectedEdgeId(null);
  }, [setEdges]);

  // Auto-layout (dagre LR)
  const formatLayout = useCallback(() => {
    setNodes((nds) => applyDagreLayout(nds, edges));
    requestAnimationFrame(() => fitView({ padding: 0.12, duration: 400 }));
  }, [edges, setNodes, fitView]);

  // Copy shareable link
  const copyLink = useCallback(() => {
    navigator.clipboard.writeText(window.location.href).then(() =>
      showToast('Link copied to clipboard!'),
    );
  }, [showToast]);

  const onPaneClick = useCallback(() => {
    setSelectedId(null);
    setSelectedEdgeId(null);
  }, []);

  const onNodeClick = useCallback((_: React.MouseEvent, node: Node) => {
    setSelectedId(node.id);
    setSelectedEdgeId(null);
  }, []);

  const onEdgeClick = useCallback((_: React.MouseEvent, edge: Edge) => {
    setSelectedId(null);
    setSelectedEdgeId(edge.id);
  }, []);

  const hasPanel = !!(selectedNode || selectedEdge);

  return (
    <div className={`${styles.workspace} ${hasPanel ? styles.hasPanel : ''}`}>
      {toast && <div className={styles.toast}>{toast}</div>}

      <div className={styles.flowContainer}>
        <ReactFlow
          nodes={nodes}
          edges={edges}
          onNodesChange={onNodesChange}
          onEdgesChange={onEdgesChange}
          onConnect={onConnect}
          onNodeClick={onNodeClick}
          onEdgeClick={onEdgeClick}
          onPaneClick={onPaneClick}
          nodeTypes={nodeTypes}
          fitView
          fitViewOptions={{ padding: 0.12 }}
          deleteKeyCode="Delete"
          proOptions={{ hideAttribution: true }}
        >
          <Background variant={BackgroundVariant.Dots} gap={18} size={1} color="var(--ifm-color-emphasis-300)" />
          <Controls />
          <MiniMap nodeColor={() => '#6366f1'} maskColor="rgba(0,0,0,0.06)" />

          <Panel position="top-left" className={styles.toolbar}>
            <button className={styles.toolbarBtn} onClick={addNode}>+ Add node</button>
            <button className={styles.toolbarBtn} onClick={groupSelected}>⬜ Group selected</button>
            <button className={styles.toolbarBtn} onClick={formatLayout}>⊞ Format</button>
            <button className={styles.toolbarBtn} onClick={copyLink}>🔗 Copy link</button>
          </Panel>
        </ReactFlow>
      </div>

      {selectedNode?.type === 'workflow' && (
        <NodeDetailPanel
          node={selectedNode as Node<WorkflowNodeData>}
          onChange={updateNode}
          onDelete={deleteNode}
          onClose={() => setSelectedId(null)}
        />
      )}
      {selectedNode?.type === 'group' && (
        <GroupDetailPanel
          node={selectedNode as Node<GroupNodeData>}
          onChange={updateGroup}
          onDelete={deleteNode}
          onClose={() => setSelectedId(null)}
        />
      )}
      {!selectedNode && selectedEdge && (
        <EdgeDetailPanel
          edge={selectedEdge}
          onToggleDelegation={toggleEdgeDelegation}
          onDelete={deleteEdge}
          onClose={() => setSelectedEdgeId(null)}
        />
      )}
    </div>
  );
}

// ── Public export (wrapped in provider) ──────────────────────────────────────

export function DagVisualizerContent() {
  return (
    <div className={styles.page}>
      <div className={styles.pageHeader}>
        <div className={styles.breadcrumb}>
          <a href="/tools">Tools</a>
          <span> / </span>
          <span>DAG Visualizer</span>
        </div>
        <h1 className={styles.pageTitle}>DAG Visualizer</h1>
        <p className={styles.pageSubtitle}>
          Map AI agent workflows as interactive graphs. Click a node to edit its agent, model, prompt, and skills.
          Click a connection to mark it as a delegation flow. Shift-click or drag-select nodes, then click "Group selected" to wrap them in a dotted box.
          Arrows can connect to group handles directly.
        </p>
      </div>

      <ReactFlowProvider>
        <DagCanvas />
      </ReactFlowProvider>
    </div>
  );
}
