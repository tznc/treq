import {useCallback, useEffect, useRef, useState, type ReactNode, type RefObject} from 'react';
import clsx from 'clsx';
import {branchStrokes, type BranchStroke} from '@site/src/lib/branchLines';
import styles from '../../pages/index.module.css';

type HeroShape = {
  width: number;
  height: number;
  left: number;
  right: number;
  y0: number;
  tipX: number;
  strokes: BranchStroke[];
};

const GAP = 26;
const BOTTOM_RADIUS = 56;
const TOP_RADIUS = 120;

/**
 * The hero's branch lines: the main line runs along the foot of the hero, and
 * the logo's three strokes leave it at one tip, rise up the empty column and
 * turn right over the product frame. Measured from the layout, so the strokes
 * always sit in the gap between the copy and the frame. Hidden below 1000px.
 */
export function HeroLines({
  heroRef,
  textRef,
  frameRef,
}: {
  heroRef: RefObject<HTMLElement | null>;
  textRef: RefObject<HTMLElement | null>;
  frameRef: RefObject<HTMLElement | null>;
}): ReactNode {
  const [shape, setShape] = useState<HeroShape | null>(null);

  const measure = useCallback(() => {
    const hero = heroRef.current;
    const text = textRef.current;
    const frame = frameRef.current;
    if (!hero || !text || !frame || window.matchMedia('(max-width: 999px)').matches) {
      setShape(null);
      return;
    }
    const h = hero.getBoundingClientRect();
    const t = text.getBoundingClientRect();
    const f = frame.getBoundingClientRect();
    const wrap = text.parentElement?.parentElement;
    const wrapBox = wrap?.getBoundingClientRect() ?? h;
    const pad = wrap ? getComputedStyle(wrap) : null;
    const left = wrapBox.left - h.left + (pad ? parseFloat(pad.paddingLeft) : 0);
    const right = wrapBox.right - h.left - (pad ? parseFloat(pad.paddingRight) : 0);
    const gutterLeft = t.right - h.left;
    const gutterRight = f.left - h.left;
    const x = Math.max(gutterLeft + 24, (gutterLeft + gutterRight) / 2 - GAP);
    const y0 = h.height - 36;
    const ytop = Math.max(40, f.top - h.top - 140);
    setShape({
      width: h.width,
      height: h.height,
      left,
      right,
      y0,
      tipX: x - BOTTOM_RADIUS,
      strokes: branchStrokes({x, y0, ytop, gap: GAP, bottomRadius: BOTTOM_RADIUS, topRadius: TOP_RADIUS, xEnd: right - 12}),
    });
  }, [heroRef, textRef, frameRef]);

  useEffect(() => {
    measure();
    const hero = heroRef.current;
    const observer = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(measure);
    if (hero) observer?.observe(hero);
    document.fonts?.ready.then(measure).catch(() => {});
    return () => observer?.disconnect();
  }, [heroRef, measure]);

  if (!shape) return <svg className={styles.heroLines} aria-hidden="true" />;
  const {left, right, y0, tipX} = shape;
  return (
    <svg className={styles.heroLines} viewBox={`0 0 ${shape.width} ${shape.height}`} aria-hidden="true">
      <line className={styles.rail} x1={left} y1={y0} x2={right} y2={y0} />
      {[0.2, 0.45, 0.7].map((f) => (
        <circle key={f} className={styles.commit} cx={left + (tipX - left) * f} cy={y0} r={8} />
      ))}
      {[...shape.strokes].reverse().map((s) => (
        <g key={s.rise} opacity={s.opacity}>
          <path className={styles.stroke} d={s.d} />
          <circle className={styles.ring} cx={s.end[0]} cy={s.end[1]} r={10} />
        </g>
      ))}
      <circle className={styles.tip} cx={tipX} cy={y0} r={9} />
    </svg>
  );
}

const WORKSPACES = ['feat/event-ingest', 'feat/keyvalues-cache', 'feat/alerting-logs'];
const OLD_TIP = 220;
const NEW_TIP = 380;

function Stack({base, ghost}: {base: number; ghost?: boolean}): ReactNode {
  const strokes = branchStrokes({x: base + 60, y0: 290, ytop: 46, gap: 40, bottomRadius: 60, topRadius: 120, xEnd: base + 380});
  return (
    <>
      {strokes.map((s, i) =>
        ghost ? (
          <path key={s.rise} className={styles.ghost} d={s.d} />
        ) : (
          <g key={s.rise}>
            <g opacity={s.opacity}>
              <path className={styles.stroke} d={s.d} />
              <circle className={styles.ring} cx={s.end[0]} cy={s.end[1]} r={10} />
            </g>
            <text className={styles.dgLabel} x={s.end[0] - 22} y={s.end[1] - 16} textAnchor="end">
              {WORKSPACES[i]}
            </text>
          </g>
        ),
      )}
    </>
  );
}

/**
 * The rebase moment: main gains two commits, and the three workspaces that
 * target it move from the old tip to the new one. It rests on the final state;
 * it plays once when it scrolls into view and again on Replay.
 */
export function RebaseDiagram(): ReactNode {
  const boxRef = useRef<HTMLDivElement>(null);
  const [pre, setPre] = useState(false);

  const play = useCallback(() => {
    if (window.matchMedia('(prefers-reduced-motion: reduce)').matches) return;
    setPre(true);
    requestAnimationFrame(() => requestAnimationFrame(() => setPre(false)));
  }, []);

  useEffect(() => {
    const box = boxRef.current;
    if (!box || typeof IntersectionObserver === 'undefined') return;
    if (window.matchMedia('(prefers-reduced-motion: reduce)').matches) return;
    if (box.getBoundingClientRect().top < window.innerHeight) return;
    setPre(true);
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((e) => e.isIntersecting)) {
          observer.disconnect();
          window.setTimeout(() => setPre(false), 250);
        }
      },
      {threshold: 0.5},
    );
    observer.observe(box);
    return () => observer.disconnect();
  }, []);

  return (
    <div ref={boxRef} className={clsx(styles.diagram, pre && styles.pre)}>
      <svg
        viewBox="0 0 800 340"
        role="img"
        aria-label="Main gains two commits. Three workspaces that target main move from the old tip to the new one.">
        <line className={styles.rail} x1={16} y1={290} x2={784} y2={290} />
        <g>
          <Stack base={OLD_TIP} ghost />
        </g>
        <g className={styles.moving}>
          <Stack base={NEW_TIP} />
        </g>
        {[60, 140, OLD_TIP].map((cx) => (
          <circle key={cx} className={styles.commit} cx={cx} cy={290} r={9} />
        ))}
        <circle className={styles.newCommit} cx={300} cy={290} r={9} />
        <circle className={styles.newCommit} cx={NEW_TIP} cy={290} r={10} />
        <text className={styles.dgNote} x={OLD_TIP} y={326} textAnchor="middle">
          old tip
        </text>
        <text className={styles.dgLabel} x={NEW_TIP} y={326} textAnchor="middle">
          main
        </text>
      </svg>
      <div className={styles.tray}>
        <span className={styles.label}>Three workspaces target main. They follow it to the new tip.</span>
        <button className={styles.replay} type="button" onClick={play}>
          Replay
        </button>
      </div>
    </div>
  );
}

/** The close: the three strokes as one rail ending on the wrap's right line. */
export function CloseRail(): ReactNode {
  return (
    <svg viewBox="0 0 1280 72" preserveAspectRatio="none" aria-hidden="true">
      {[1, 0.72, 0.46].map((opacity, i) => (
        <path
          key={opacity}
          className={styles.stroke}
          d={`M 6 ${12 + i * 24} H 1274`}
          opacity={opacity}
          vectorEffect="non-scaling-stroke"
        />
      ))}
    </svg>
  );
}
