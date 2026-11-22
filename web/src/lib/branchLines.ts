// The logo's three strokes, drawn as branch lines: each leaves the main line at
// one tip commit, rises, turns right and runs to an end line. Stroke i rises
// and runs one gap inside stroke i-1, so the three nest like the mark.

export type BranchGeometry = {
  /** x where the outer stroke rises */
  x: number;
  /** y of the main line */
  y0: number;
  /** y where the outer stroke runs to the end line */
  ytop: number;
  /** distance between strokes */
  gap: number;
  /** radius of the outer stroke's turn off the main line */
  bottomRadius: number;
  /** radius of the outer stroke's turn to the right */
  topRadius: number;
  /** x where every stroke ends */
  xEnd: number;
};

export type BranchStroke = {
  d: string;
  rise: number;
  end: [number, number];
  topRadius: number;
  opacity: number;
};

// Control-point distance for a quarter circle drawn as one cubic Bézier.
const K = 0.5523;
const OPACITY = [1, 0.72, 0.46];
const round = (n: number) => Math.round(n * 100) / 100;

export function branchStrokes(g: BranchGeometry): BranchStroke[] {
  return OPACITY.map((opacity, i) => {
    const x = g.x + i * g.gap;
    const y = g.ytop + i * g.gap;
    const top = Math.max(0, g.topRadius - i * g.gap);
    const bottom = g.bottomRadius + i * g.gap;
    const p = (...n: number[]) => n.map(round).join(' ');
    const d = [
      `M ${p(x - bottom, g.y0)}`,
      `C ${p(x - bottom + K * bottom, g.y0, x, g.y0 - bottom + K * bottom, x, g.y0 - bottom)}`,
      `L ${p(x, y + top)}`,
      `C ${p(x, y + top - K * top, x + top - K * top, y, x + top, y)}`,
      `L ${p(g.xEnd, y)}`,
    ].join(' ');
    return {d, rise: x, end: [g.xEnd, y], topRadius: top, opacity};
  });
}
