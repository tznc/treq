import type React from 'react';
import { createContext, useContext, useState } from 'react';
import type { Database } from 'sql.js';

type SearchDbContextValue = {
  db: Database | null;
  setDb: (db: Database) => void;
};

const SearchDbContext = createContext<SearchDbContextValue>({
  db: null,
  setDb: () => {},
});

// Intentionally does NOT eagerly load the search DB on mount. Loading sql.js
// WASM + the full SQLite blob on every page (including the homepage) competes
// with the LCP image for bandwidth and blocks the main thread during initial
// render. SearchBar lazily loads/pre-warms the DB itself on focus/query via
// getDb() in ./searchDb, then calls setDb() to publish the result here so any
// other consumer (e.g. a search box mounted on a later navigation) reuses the
// already-loaded DB instead of re-fetching/re-instantiating it.
export function SearchDbProvider({ children }: { children: React.ReactNode }) {
  const [db, setDb] = useState<Database | null>(null);

  return (
    <SearchDbContext.Provider value={{ db, setDb }}>{children}</SearchDbContext.Provider>
  );
}

export function useSearchDb(): Database | null {
  return useContext(SearchDbContext).db;
}

export function useSetSearchDb(): (db: Database) => void {
  return useContext(SearchDbContext).setDb;
}
