import {
  createContext,
  useContext,
  useLayoutEffect,
  type ReactNode,
} from "react";
import {
  type ActiveRepository,
  repositoryCacheKey,
  setActiveRepositorySingleton,
} from "./active-repository";

const ActiveRepositoryContext = createContext<ActiveRepository | null>(null);

export function ActiveRepositoryProvider({
  repository,
  children,
}: {
  repository: ActiveRepository | null;
  children: ReactNode;
}) {
  useLayoutEffect(() => {
    setActiveRepositorySingleton(repository);
    return () => setActiveRepositorySingleton(null);
  }, [repository]);

  return (
    <ActiveRepositoryContext.Provider value={repository}>
      {children}
    </ActiveRepositoryContext.Provider>
  );
}

export function useActiveRepository(): ActiveRepository | null {
  return useContext(ActiveRepositoryContext);
}

export function useRepositoryCacheKey(
  fallbackPath?: string,
): string | undefined {
  const repo = useActiveRepository();
  if (repo) return repositoryCacheKey(repo);
  return fallbackPath;
}
