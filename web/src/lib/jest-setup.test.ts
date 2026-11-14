// Proves the Jest + TypeScript setup runs. Remove once real unit tests exist.
const add = (a: number, b: number): number => a + b;

test('runs TypeScript tests', () => {
  expect(add(1, 2)).toBe(3);
});
