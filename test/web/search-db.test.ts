import { score } from "../../web/src/utils/searchDb";

// matchinfo('pcx') for 3 phrases x 3 columns (title, body, url):
// per phrase and column, [hits in this row, hits in all rows, rows with a hit].
function matchinfo(cells: [number, number, number][]): Uint8Array {
  return new Uint8Array(new Uint32Array([3, 3, ...cells.flat()]).buffer);
}

describe("search score", () => {
  it("ranks a page that matches every term in its title and body above one that mentions each once", () => {
    const strong = matchinfo([
      [1, 1, 1],
      [3, 12, 11],
      [1, 1, 1],
      [0, 0, 0],
      [3, 12, 11],
      [0, 0, 0],
      [0, 0, 0],
      [3, 12, 11],
      [0, 0, 0],
    ]);
    const weak = matchinfo([
      [0, 1, 1],
      [1, 12, 11],
      [0, 1, 1],
      [0, 0, 0],
      [1, 12, 11],
      [0, 0, 0],
      [0, 0, 0],
      [1, 12, 11],
      [0, 0, 0],
    ]);
    expect(score(strong)).toBeGreaterThan(score(weak));
  });

  it("weights a title hit above the same hit in the body", () => {
    const inTitle = matchinfo([
      [1, 2, 2],
      [0, 2, 2],
      [0, 0, 0],
      ...Array(6).fill([0, 0, 0]),
    ]);
    const inBody = matchinfo([
      [0, 2, 2],
      [1, 2, 2],
      [0, 0, 0],
      ...Array(6).fill([0, 0, 0]),
    ]);
    expect(score(inTitle)).toBeGreaterThan(score(inBody));
  });

  it("scores a row with no hits as zero", () => {
    expect(score(matchinfo(Array(9).fill([0, 4, 4])))).toBe(0);
  });
});
