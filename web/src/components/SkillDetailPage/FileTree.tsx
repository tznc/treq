import type React from 'react';
import { useMemo, useState } from 'react';
import type { SkillFile } from './types';
import styles from './styles.module.css';

interface TreeNode {
  name: string;
  path: string;
  type: 'folder' | 'file';
  file?: SkillFile;
  children?: TreeNode[];
}

function buildTree(files: SkillFile[]): TreeNode[] {
  const root: TreeNode = { name: '', path: '', type: 'folder', children: [] };
  for (const file of files) {
    const segments = file.path.split('/');
    let current = root;
    segments.forEach((segment, index) => {
      const isFile = index === segments.length - 1;
      const childPath = current.path ? `${current.path}/${segment}` : segment;
      current.children ??= [];
      let child = current.children.find((n) => n.name === segment);
      if (!child) {
        child = isFile
          ? { name: segment, path: childPath, type: 'file', file }
          : { name: segment, path: childPath, type: 'folder', children: [] };
        current.children.push(child);
      }
      if (!isFile) current = child;
    });
  }
  return root.children ?? [];
}

function sortTree(nodes: TreeNode[]): TreeNode[] {
  return nodes
    .map((n) => (n.children ? { ...n, children: sortTree(n.children) } : n))
    .sort((a, b) => (a.type === b.type ? a.name.localeCompare(b.name) : a.type === 'folder' ? -1 : 1));
}

// Auto-expand only top-level folders, so skills with deeply nested reference
// directories (e.g. office schemas) don't open into an overwhelming tree.
function collectTopLevelFolderPaths(nodes: TreeNode[]): string[] {
  return nodes.filter((n) => n.type === 'folder').map((n) => n.path);
}

export function FileTree({
  files,
  selectedPath,
  onSelect,
}: {
  files: SkillFile[];
  selectedPath: string | null;
  onSelect: (file: SkillFile) => void;
}) {
  const tree = useMemo(() => sortTree(buildTree(files)), [files]);
  const [expanded, setExpanded] = useState<Set<string>>(() => new Set(collectTopLevelFolderPaths(tree)));

  const toggle = (path: string) => {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
  };

  const renderNode = (node: TreeNode, depth: number): React.ReactNode => {
    if (node.type === 'folder') {
      const isOpen = expanded.has(node.path);
      return (
        <div key={node.path}>
          <button
            type="button"
            className={styles.treeFolder}
            style={{ paddingLeft: `${depth * 14 + 8}px` }}
            onClick={() => toggle(node.path)}
          >
            <span className={styles.treeIcon}>{isOpen ? '▾' : '▸'}</span>
            {node.name}
          </button>
          {isOpen && node.children?.map((child) => renderNode(child, depth + 1))}
        </div>
      );
    }
    return (
      <button
        key={node.path}
        type="button"
        className={node.path === selectedPath ? styles.treeFileActive : styles.treeFile}
        style={{ paddingLeft: `${depth * 14 + 8}px` }}
        onClick={() => node.file && onSelect(node.file)}
      >
        {node.name}
      </button>
    );
  };

  return <div className={styles.treeInner}>{tree.map((node) => renderNode(node, 0))}</div>;
}
