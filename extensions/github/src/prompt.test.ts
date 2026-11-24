import { describe, expect, it } from "vitest";
import { codePrompt } from "./prompt";

const at = (lines: number[]) => ({
  repo: "tznc/treq",
  ref: "main",
  path: "src/a.rs",
  lines: lines.map((line) => ({ line, text: `code ${line}` })),
});

describe("codePrompt", () => {
  it("leads with the file, line and code", () => {
    expect(codePrompt(at([4]), "why?")).toBe(
      "src/a.rs:4 (tznc/treq@main)\n```\n4  code 4\n```\n\nwhy?",
    );
  });

  it("numbers each line of a range", () => {
    expect(codePrompt(at([9, 10]), "x")).toBe(
      "src/a.rs:9-10 (tznc/treq@main)\n```\n 9  code 9\n10  code 10\n```\n\nx",
    );
  });

  it("embeds at most 10 lines, eliding the middle", () => {
    const prompt = codePrompt(at([1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]), "x");
    expect(prompt.split("\n").slice(0, 14)).toEqual([
      "src/a.rs:1-12 (tznc/treq@main)",
      "```",
      " 1  code 1",
      " 2  code 2",
      " 3  code 3",
      " 4  code 4",
      " 5  code 5",
      "…",
      " 8  code 8",
      " 9  code 9",
      "10  code 10",
      "11  code 11",
      "12  code 12",
      "```",
    ]);
  });
});
