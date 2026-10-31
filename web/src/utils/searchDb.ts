// Fetch the SQLite DB once as a binary blob and query it with sql.js in-memory.
// Lazy: init only when preWarm() or getDb() is first called.

import type { Database } from 'sql.js';

let dbPromise: Promise<Database> | null = null;

export function getDb(): Promise<Database> {
  if (!dbPromise) {
    dbPromise = (async () => {
      const [{ url }, initSqlJs] = await Promise.all([
        fetch('/search-meta.json').then(r => r.json()) as Promise<{ url: string }>,
        import('sql.js').then(m => m.default ?? m),
      ]);
      const [SQL, buf] = await Promise.all([
        initSqlJs({ locateFile: (f: string) => `/${f}` }),
        fetch(url).then(r => r.arrayBuffer()),
      ]);
      return new SQL.Database(new Uint8Array(buf));
    })().catch(err => { dbPromise = null; throw err; });
  }
  return dbPromise;
}

export function preWarm(): void {
  getDb().catch(() => {});
}

export type SearchResult = { title: string; url: string; excerpt: string };

const MAX_RESULTS = 8;
// Column weights for title, body and url: a hit in the title counts most.
const WEIGHTS = [5, 1, 0];

// FTS4 has no bm25, so rank by matchinfo('pcx'): [phrases, columns, then per
// phrase and column: hits in this row, hits in all rows, rows with a hit].
// Each hit is weighted by column and by how rare the term is overall.
function score(blob: Uint8Array): number {
  const m = new Uint32Array(blob.buffer, blob.byteOffset, blob.byteLength / 4);
  const [phrases, cols] = m;
  let total = 0;
  for (let p = 0; p < phrases; p++) {
    for (let c = 0; c < cols; c++) {
      const i = 2 + 3 * (p * cols + c);
      if (m[i]) total += (WEIGHTS[c] ?? 1) * m[i] / m[i + 1];
    }
  }
  return total;
}

export function execSearch(db: Database, q: string): SearchResult[] {
  const term = q.trim().split(/\s+/).filter(Boolean).map(w => `${w}*`).join(' ');
  if (!term) return [];
  const res = db.exec(
    `SELECT snippet(docs_fts, '<mark>', '</mark>', '', 0, 10) AS title,
            url,
            snippet(docs_fts, '<mark>', '</mark>', '…', 1, 20) AS excerpt,
            matchinfo(docs_fts, 'pcx') AS info
     FROM docs_fts WHERE docs_fts MATCH ?`,
    [term],
  );
  if (!res.length) return [];
  return res[0].values
    .map(([title, url, excerpt, info]) => ({
      result: { title, url, excerpt } as SearchResult,
      rank: score(info as Uint8Array),
    }))
    .sort((a, b) => b.rank - a.rank)
    .slice(0, MAX_RESULTS)
    .map(r => r.result);
}
