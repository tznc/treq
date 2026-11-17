import type React from 'react';
import { useState, useEffect, useRef, useCallback } from 'react';
import BrowserOnly from '@docusaurus/BrowserOnly';
import Link from '@docusaurus/Link';
import { useHistory } from '@docusaurus/router';
import { getDb, execSearch, type SearchResult } from '@site/src/utils/searchDb';
import { useSearchDb, useSetSearchDb } from '@site/src/utils/SearchDbContext';
import styles from './styles.module.css';

function SearchBarInner() {
  const [query, setQuery] = useState('');
  const [results, setResults] = useState<SearchResult[]>([]);
  const [open, setOpen] = useState(false);
  const containerRef = useRef<HTMLDivElement>(null);
  const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const db = useSearchDb();
  const setDb = useSetSearchDb();
  const history = useHistory();

  const runQuery = useCallback(async (q: string) => {
    if (!q.trim()) { setResults([]); setOpen(false); return; }
    try {
      let resolvedDb = db;
      if (!resolvedDb) {
        resolvedDb = await getDb();
        setDb(resolvedDb);
      }
      const rows = execSearch(resolvedDb, q);
      setResults(rows);
      setOpen(rows.length > 0);
    } catch (err) {
      console.error('[SearchBar] query failed:', err);
      setResults([]);
      setOpen(false);
    }
  }, []);

  useEffect(() => {
    if (debounceRef.current) clearTimeout(debounceRef.current);
    debounceRef.current = setTimeout(() => runQuery(query), 300);
    return () => { if (debounceRef.current) clearTimeout(debounceRef.current); };
  }, [query, runQuery]);

  useEffect(() => {
    function onKey(e: KeyboardEvent) { if (e.key === 'Escape') setOpen(false); }
    function onClickOutside(e: MouseEvent) {
      if (containerRef.current && !containerRef.current.contains(e.target as Node)) setOpen(false);
    }
    document.addEventListener('keydown', onKey);
    document.addEventListener('mousedown', onClickOutside);
    return () => {
      document.removeEventListener('keydown', onKey);
      document.removeEventListener('mousedown', onClickOutside);
    };
  }, []);

  function handleKeyDown(e: React.KeyboardEvent<HTMLInputElement>) {
    if (e.key === 'Enter' && query.trim()) {
      setOpen(false);
      history.push(`/search?q=${encodeURIComponent(query.trim())}`);
    }
  }

  return (
    <div className={styles.container} ref={containerRef}>
      <input
        className={styles.input}
        type="search"
        placeholder="Search..."
        value={query}
        onChange={e => setQuery(e.target.value)}
        onKeyDown={handleKeyDown}
        onFocus={() => {
          if (!db) getDb().then(setDb).catch(() => {});
          if (results.length > 0) setOpen(true);
        }}
        aria-label="Search..."
      />
      {open && (
        <div className={styles.dropdown}>
          {results.map((r, i) => (
            <Link
              key={i}
              to={r.url}
              className={styles.result}
              onClick={() => { setOpen(false); setQuery(''); }}
            >
              <div
                className={styles.resultTitle}
                dangerouslySetInnerHTML={{ __html: r.title || r.url }}
              />
              {r.excerpt && (
                <div
                  className={styles.resultExcerpt}
                  dangerouslySetInnerHTML={{ __html: r.excerpt }}
                />
              )}
            </Link>
          ))}
          {query.trim() && (
            <Link
              to={`/search?q=${encodeURIComponent(query.trim())}`}
              className={styles.seeAll}
              onClick={() => setOpen(false)}
            >
              See all results for "{query}"
            </Link>
          )}
        </div>
      )}
    </div>
  );
}

export default function SearchBar() {
  return <BrowserOnly>{() => <SearchBarInner />}</BrowserOnly>;
}
