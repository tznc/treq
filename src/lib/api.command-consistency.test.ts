import { describe, expect, it } from "vitest";
import fs from "fs";
import path from "path";

const ROOT = path.resolve(__dirname, "../..");

function readFile(relative: string): string {
  return fs.readFileSync(path.join(ROOT, relative), "utf-8");
}

function extractApiCommands(content: string): Set<string> {
  const commands = new Set<string>();
  for (const match of content.matchAll(/invoke\(\s*["'](\w+)["']/g)) {
    commands.add(match[1]);
  }
  return commands;
}

function extractTauriCommands(content: string): Set<string> {
  const commands = new Set<string>();
  for (const match of content.matchAll(/commands::(\w+)/g)) {
    commands.add(match[1]);
  }
  return commands;
}

function setDiff<T>(setA: Set<T>, setB: Set<T>): T[] {
  return [...setA].filter((elem) => !setB.has(elem)).sort() as T[];
}

describe("command consistency", () => {
  const apiContent = readFile("src/lib/api.ts");
  const tauriLibContent = readFile("src-tauri/src/lib.rs");

  const apiCommands = extractApiCommands(apiContent);
  const tauriCommands = extractTauriCommands(tauriLibContent);

  it("every invoke() command in api.ts is registered in Tauri generate_handler!", () => {
    const missing = setDiff(apiCommands, tauriCommands);
    expect(
      missing,
      `Commands in api.ts missing from src-tauri/src/lib.rs generate_handler!:\n  ${missing.join(
        "\n  ",
      )}`,
    ).toEqual([]);
  });
});
