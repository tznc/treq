// Concurrency caps for drain and retire migrations
// (docs/design/sandbox-provider-adapter.md §9, `drain_tick`). Every open
// migration counts, parked ones included, so a drain cannot flood one
// provider's create API or the whole fleet at once.

export interface MigrationCaps {
  perProvider: number;
  total: number;
}

const DEFAULT_PER_PROVIDER = 10;
const DEFAULT_TOTAL = 50;

function positiveInt(value: string | undefined, fallback: number): number {
  if (value === undefined || !/^\d+$/.test(value)) return fallback;
  const n = Number(value);
  return n > 0 ? n : fallback;
}

export function migrationCaps(
  env: (key: string) => string | undefined = (key) => Deno.env.get(key),
): MigrationCaps {
  return {
    perProvider: positiveInt(env("TREQ_MIGRATION_MAX_OPEN_PER_PROVIDER"), DEFAULT_PER_PROVIDER),
    total: positiveInt(env("TREQ_MIGRATION_MAX_OPEN"), DEFAULT_TOTAL),
  };
}

/** How many more drain migrations may be created for one provider now. */
export function drainAllowance(p: {
  batch: number;
  openForProvider: number;
  openTotal: number;
  caps: MigrationCaps;
}): number {
  return Math.max(0, Math.min(p.batch, p.caps.perProvider - p.openForProvider, p.caps.total - p.openTotal));
}
