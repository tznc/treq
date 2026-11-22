export type QuarterId = 'Q1' | 'Q2' | 'Q3' | 'Q4';

export type QuarterIcon = 'empty' | 'mergeChecks' | 'ssh';

export interface MilestoneMeta {
  id: string;
  title: string;
  shortTitle: string;
  href: string;
  outcome: string;
}

export interface QuarterSlot {
  id: QuarterId;
  icon: QuarterIcon;
  milestones: MilestoneMeta[];
}

export interface YearRoadmap {
  year: number;
  /** Newest years should be prepended to ROADMAP_YEARS. */
  quarters: [QuarterSlot, QuarterSlot, QuarterSlot, QuarterSlot];
}

/**
 * Year roadmaps, newest first. Prepend a new year object when that year opens.
 * Leave a quarter's `milestones` empty to grey it out in the visualization.
 */
export const ROADMAP_YEARS: YearRoadmap[] = [
  {
    year: 2026,
    quarters: [
      { id: 'Q1', icon: 'empty', milestones: [] },
      { id: 'Q2', icon: 'empty', milestones: [] },
      {
        id: 'Q3',
        icon: 'mergeChecks',
        milestones: [
          {
            id: 'merge',
            title: 'GitHub Integration',
            shortTitle: 'GitHub PRs',
            href: '#github-integration',
            outcome:
              'Open and track stacked pull requests from the Treq app, with CI status and review threads next to the workspace.',
          },
          {
            id: 'checks',
            title: 'Workspace Checks',
            shortTitle: 'Workspace Checks',
            href: '#workspace-checks',
            outcome:
              'Define and run verification workflows per workspace so agents can iterate against hard pass/fail gates.',
          },
        ],
      },
      {
        id: 'Q4',
        icon: 'ssh',
        milestones: [
          {
            id: 'ssh',
            title: 'SSH Remote Development',
            shortTitle: 'SSH Remote',
            href: '#ssh-remote-development',
            outcome: 'Open a remote workspace over SSH and develop there directly from Treq.',
          },
        ],
      },
    ],
  },
];

export function quarterLabel(year: number, quarter: QuarterId): string {
  return `${quarter} ${year}`;
}

export function isQuarterPlanned(slot: QuarterSlot): boolean {
  return slot.milestones.length > 0;
}

export function firstPlannedIndex(year: YearRoadmap): number {
  const idx = year.quarters.findIndex(isQuarterPlanned);
  return idx === -1 ? 0 : idx;
}

export function nextPlannedIndex(year: YearRoadmap, from: number): number {
  for (let step = 1; step <= 4; step++) {
    const idx = (from + step) % 4;
    const slot = year.quarters[idx];
    if (slot && isQuarterPlanned(slot)) return idx;
  }
  return from;
}

export function getYearRoadmap(version: string | number): YearRoadmap | undefined {
  const year = typeof version === 'number' ? version : Number.parseInt(version, 10);
  return ROADMAP_YEARS.find((entry) => entry.year === year);
}
