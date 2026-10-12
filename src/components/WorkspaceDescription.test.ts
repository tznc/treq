import { describe, expect, it } from "vitest";
import { looksLikeMarkdown } from "./WorkspaceDescription";

describe("looksLikeMarkdown", () => {
  it.each([
    "## Summary",
    "- one\n- two",
    "1. first\n2. second",
    "Fixes **the** bug",
    "See [the docs](https://treq.dev)",
    "```\ncode\n```",
    "> quoted",
    "| a | b |\n| - | - |",
  ])("detects %j", (text) => {
    expect(looksLikeMarkdown(text)).toBe(true);
  });

  it.each([
    "Fix the flaky login test",
    "Retry on 5xx - then give up",
    "C# interop #42",
    "2*3 = 6",
  ])("treats %j as plain text", (text) => {
    expect(looksLikeMarkdown(text)).toBe(false);
  });
});
