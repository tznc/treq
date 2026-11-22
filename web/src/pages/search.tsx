import React, { useState, useEffect } from 'react';
import Layout from '@theme/Layout';
import Link from '@docusaurus/Link';
import BrowserOnly from '@docusaurus/BrowserOnly';
import { getDb, execSearch, type SearchResult } from '@site/src/utils/searchDb';
import { useSearchDb } from '@site/src/utils/SearchDbContext';
import styles from './search.module.css';

function SearchResults({ query }: { query: string }) {
  const db = useSearchDb();
  const [results, setResults] = useState<SearchResult[]>([]);
  const [loading, setLoading] = useState(() => !!query.trim());
  const [searched, setSearched] = useState(false);

  useEffect(() => {
    if (!query.trim()) return;
    setLoading(true);
    setSearched(false);
    const run = async () => {
      const resolvedDb = db ?? await getDb();
      return execSearch(resolvedDb, query);
    };
    run()
      .then(rows => { setResults(rows); setSearched(true); })
      .catch(err => { console.error('[Search] query failed:', err); setResults([]); setSearched(true); })
      .finally(() => setLoading(false));
  }, [query, db]);

  if (!query.trim()) return null;
  if (loading) return <p className={styles.status}>Searching…</p>;
  if (searched && results.length === 0) {
    return <p className={styles.status}>No results found for "<strong>{query}</strong>".</p>;
  }

  return (
    <ul className={styles.results}>
      {results.map((r, i) => (
        <li key={i} className={styles.result}>
          <Link
            to={r.url}
            className={styles.resultTitle ?? ''}
            dangerouslySetInnerHTML={{ __html: r.title || r.url }}
          />
          <div className={styles.resultUrl}>{r.url}</div>
          {r.excerpt && (
            <p
              className={styles.resultExcerpt}
              dangerouslySetInnerHTML={{ __html: r.excerpt }}
            />
          )}
        </li>
      ))}
    </ul>
  );
}

function SearchPage() {
  const params = new URLSearchParams(window.location.search);
  const query = params.get('q') ?? '';

  return (
    <div className={styles.container}>
      <h1 className={styles.heading}>
        {query ? <>Search results for "<strong>{query}</strong>"</> : 'Search'}
      </h1>
      <SearchResults query={query} />
    </div>
  );
}

export default function Search() {
  return (
    <Layout title="Search" description="Search the Treq documentation">
      <main className={styles.main}>
        <BrowserOnly>{() => <SearchPage />}</BrowserOnly>
      </main>
    </Layout>
  );
}
