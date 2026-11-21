import {describe, expect, it} from '@jest/globals';
import {branchStrokes, type BranchGeometry} from './branchLines';

const geometry: BranchGeometry = {x: 400, y0: 600, ytop: 100, gap: 30, bottomRadius: 60, topRadius: 120, xEnd: 1200};

// Pulls every number out of an SVG path string, in order.
const numbers = (d: string) => (d.match(/-?\d+(\.\d+)?/g) ?? []).map(Number);

describe('branchStrokes', () => {
  it('draws the logo three strokes, strongest first', () => {
    const strokes = branchStrokes(geometry);
    expect(strokes).toHaveLength(3);
    expect(strokes.map((s) => s.opacity)).toEqual([1, 0.72, 0.46]);
  });

  it('leaves the main line from one tip commit', () => {
    const starts = branchStrokes(geometry).map((s) => numbers(s.d).slice(0, 2));
    for (const start of starts) {
      expect(start).toEqual([geometry.x - geometry.bottomRadius, geometry.y0]);
    }
  });

  it('nests the strokes: each rises and runs one gap inside the last', () => {
    const strokes = branchStrokes(geometry);
    strokes.forEach((s, i) => {
      expect(s.rise).toBe(geometry.x + i * geometry.gap);
      expect(s.end).toEqual([geometry.xEnd, geometry.ytop + i * geometry.gap]);
    });
  });

  it('ends every stroke on the end line', () => {
    for (const s of branchStrokes(geometry)) {
      const n = numbers(s.d);
      expect(n.slice(-2)).toEqual(s.end);
    }
  });

  it('never gives a corner a negative radius', () => {
    const tight = branchStrokes({...geometry, topRadius: 40, gap: 30});
    for (const s of tight) {
      expect(numbers(s.d).every(Number.isFinite)).toBe(true);
      expect(s.topRadius).toBeGreaterThanOrEqual(0);
    }
  });
});
