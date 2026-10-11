import { describe, expect, it } from "vitest";
import { resizeTerminalPair } from "./resizeTerminalPair";

describe("resizeTerminalPair", () => {
  it("moves width from one column to the other", () => {
    expect(
      resizeTerminalPair({
        leftWidth: 500,
        rightWidth: 500,
        deltaX: 50,
        minWidth: 400,
      }),
    ).toEqual({
      left: 550,
      right: 450,
    });
  });

  it("grows the left column past a right column already at the minimum", () => {
    expect(
      resizeTerminalPair({
        leftWidth: 400,
        rightWidth: 400,
        deltaX: 50,
        minWidth: 400,
      }),
    ).toEqual({
      left: 450,
      right: 400,
    });
  });

  it("does not shrink the left column below the minimum", () => {
    expect(
      resizeTerminalPair({
        leftWidth: 400,
        rightWidth: 400,
        deltaX: -50,
        minWidth: 400,
      }),
    ).toEqual({
      left: 400,
      right: 400,
    });
    expect(
      resizeTerminalPair({
        leftWidth: 450,
        rightWidth: 400,
        deltaX: -100,
        minWidth: 400,
      }),
    ).toEqual({
      left: 400,
      right: 450,
    });
  });
});
