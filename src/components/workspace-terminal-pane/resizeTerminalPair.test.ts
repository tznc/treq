import { describe, expect, it } from "vitest";
import { resizeTerminalPair } from "./resizeTerminalPair";

describe("resizeTerminalPair", () => {
  it("moves width from one column to the other", () => {
    expect(resizeTerminalPair(500, 500, 50, 400)).toEqual({
      left: 550,
      right: 450,
    });
  });

  it("grows the left column past a right column already at the minimum", () => {
    expect(resizeTerminalPair(400, 400, 50, 400)).toEqual({
      left: 450,
      right: 400,
    });
  });

  it("does not shrink the left column below the minimum", () => {
    expect(resizeTerminalPair(400, 400, -50, 400)).toEqual({
      left: 400,
      right: 400,
    });
    expect(resizeTerminalPair(450, 400, -100, 400)).toEqual({
      left: 400,
      right: 450,
    });
  });
});
