import type React from 'react';
import { useEffect, useRef, useState, useCallback, useMemo } from 'react';
import {
  getYearRoadmap,
  firstPlannedIndex,
  isQuarterPlanned,
  quarterLabel,
} from './_roadmapData';
import {
  buildRoadmapScene,
  type RoadmapSceneAPI,
} from './_roadmapScene';
import styles from './styles.module.css';

function useIsDark(): boolean {
  const [dark, setDark] = useState(false);

  useEffect(() => {
    const read = () =>
      document.documentElement.getAttribute('data-theme') === 'dark';
    setDark(read());

    const observer = new MutationObserver(() => setDark(read()));
    observer.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ['data-theme'],
    });
    return () => observer.disconnect();
  }, []);

  return dark;
}

export interface RoadmapGraphicTabsProps {
  /** Calendar year to render, for example `"2026"`. */
  version: string | number;
}

export default function RoadmapGraphicTabs({
  version,
}: RoadmapGraphicTabsProps): React.ReactElement | null {
  const year = useMemo(() => getYearRoadmap(version), [version]);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const sceneRef = useRef<RoadmapSceneAPI | null>(null);
  const dark = useIsDark();
  const [active, setActive] = useState(0);
  const [ready, setReady] = useState(false);

  useEffect(() => {
    if (!year) return;
    setActive(firstPlannedIndex(year));
  }, [year]);

  const plannedTabs = useMemo(() => {
    if (!year) return [];
    return year.quarters
      .map((item, index) => ({ item, index }))
      .filter(({ item }) => isQuarterPlanned(item));
  }, [year]);

  useEffect(() => {
    if (!year) return;
    const canvas = canvasRef.current;
    if (!canvas) return;

    let mounted = true;
    const reducedMotion = window.matchMedia('(prefers-reduced-motion: reduce)').matches;

    buildRoadmapScene(canvas, {
      year,
      dark,
      reducedMotion,
      onActiveChange: (index) => {
        if (mounted) setActive(index);
      },
    }).then((api) => {
      if (!mounted) {
        api.dispose();
        return;
      }
      sceneRef.current = api;
      setReady(true);
    });

    return () => {
      mounted = false;
      sceneRef.current?.dispose();
      sceneRef.current = null;
      setReady(false);
    };
  }, [dark, year]);

  const select = useCallback((index: number) => {
    setActive(index);
    sceneRef.current?.setActive(index);
  }, []);

  if (!year) {
    return (
      <p className={styles.missing}>
        No roadmap data for {String(version)}.
      </p>
    );
  }

  const slot = year.quarters[active];
  const planned = isQuarterPlanned(slot);

  return (
    <div className={styles.stage} aria-label={`${year.year} roadmap timeline`}>
      <div className={styles.viz}>
        <canvas
          ref={canvasRef}
          className={`${styles.canvas}${ready ? ` ${styles.canvasReady}` : ''}`}
          aria-hidden="true"
        />
      </div>

      <div className={styles.table} role="tablist" aria-label={`${year.year} planned quarters`}>
        {plannedTabs.map(({ item, index }) => {
          const label = item.milestones.map((m) => m.shortTitle).join(', ');
          return (
            <button
              key={item.id}
              type="button"
              role="tab"
              aria-selected={index === active}
              className={`${styles.row}${index === active ? ` ${styles.rowActive}` : ''}`}
              onClick={() => select(index)}
            >
              <span className={styles.rowQuarter}>{quarterLabel(year.year, item.id)}</span>
              <span className={styles.rowTitle}>{label}</span>
            </button>
          );
        })}
      </div>

      <div className={styles.outcome} aria-live="polite" key={`${year.year}-${slot.id}`}>
        {!planned ? (
          <>
            <p className={styles.outcomeTitleText}>
              {quarterLabel(year.year, slot.id)}
            </p>
            <p className={styles.outcomeText}>Nothing planned for this quarter.</p>
          </>
        ) : (
          <ul className={styles.outcomeList}>
            {slot.milestones.map((milestone) => (
              <li key={milestone.id} className={styles.outcomeItem}>
                <a className={styles.outcomeTitle} href={milestone.href}>
                  {milestone.title}
                </a>
                <p className={styles.outcomeText}>{milestone.outcome}</p>
              </li>
            ))}
          </ul>
        )}
      </div>
    </div>
  );
}
