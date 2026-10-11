/**
 * Moves the divider between two adjacent terminal columns by `deltaX`.
 *
 * The left column never drops below `minWidth`. Width the left column gains
 * comes out of the right one until the right one reaches `minWidth`; past
 * that the left column keeps growing and the pane scrolls horizontally.
 * Without that overflow, a pane whose columns all sit at `minWidth` (three or
 * more terminals) could not be resized at all.
 */
export function resizeTerminalPair({
  leftWidth,
  rightWidth,
  deltaX,
  minWidth,
}: {
  leftWidth: number;
  rightWidth: number;
  deltaX: number;
  minWidth: number;
}): { left: number; right: number } {
  const left = Math.max(minWidth, leftWidth + deltaX);
  const right = Math.max(minWidth, rightWidth - (left - leftWidth));
  return { left, right };
}
