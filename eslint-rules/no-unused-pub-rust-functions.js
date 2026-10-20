import { execFileSync } from "node:child_process";
import { existsSync, readFileSync, readdirSync } from "node:fs";
import { dirname, join } from "node:path";

const REPORT_FILE = join("src", "lib", "api.ts");
const RUST_DIRS = [join("src-tauri", "src")];
// The mobile app shares `src-tauri/src/core`, so its uses count too.
const REFERENCE_DIRS = [...RUST_DIRS, join("mobile-gpui", "src")];
const AST_GREP_RULE = join(
  "test",
  "ast-grep",
  "utils",
  "unused-pub-rust-function-candidate.yml",
);
const FUNCTION_NAME_REGEX = /^pub(?:\s+async)?\s+fn\s+([A-Za-z_]\w*)\b/m;

function collectRustFiles(dir, out = []) {
  if (!existsSync(dir)) {
    return out;
  }

  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) {
      collectRustFiles(path, out);
      continue;
    }
    if (entry.isFile() && entry.name.endsWith(".rs")) {
      out.push(path);
    }
  }

  return out;
}

function countIdentifier(text, name) {
  const matches = text.match(new RegExp(`\\b${name}\\b`, "g"));
  return matches ? matches.length : 0;
}

function getAstGrepCommand(rootDir) {
  const localBin = join(
    rootDir,
    "node_modules",
    ".bin",
    process.platform === "win32" ? "ast-grep.cmd" : "ast-grep",
  );
  return existsSync(localBin) ? localBin : "ast-grep";
}

function loadRustText(rootDir) {
  return REFERENCE_DIRS.flatMap((dir) => collectRustFiles(join(rootDir, dir)))
    .map((file) => readFileSync(file, "utf8"))
    .join("\n");
}

function collectPublicRustFunctions(rootDir) {
  const command = getAstGrepCommand(rootDir);
  const args = [
    "scan",
    ...RUST_DIRS,
    "--rule",
    AST_GREP_RULE,
    "--json=compact",
    "--color=never",
  ];
  const output = execFileSync(command, args, {
    cwd: rootDir,
    encoding: "utf8",
    maxBuffer: 16 * 1024 * 1024,
  });
  const matches = JSON.parse(output || "[]");

  return matches.flatMap((match) => {
    if ((match.charCount?.leading ?? 0) !== 0) {
      return [];
    }

    const name = match.text.match(FUNCTION_NAME_REGEX)?.[1];
    if (!name) {
      return [];
    }

      return [
        {
          file: match.file,
          line: (match.range?.start?.line ?? 0) + 1,
          name,
        },
      ];
  });
}

export default {
  meta: {
    type: "problem",
    docs: {
      description:
        "Identify top-level public Rust functions under src-tauri/src that have no in-repo references",
      recommended: false,
    },
    schema: [],
    messages: {
      astGrepFailure:
        "Failed to scan Rust public functions with ast-grep: {{error}}",
      unusedPublicFunction:
        'Unused public Rust function "{{name}}" declared at {{file}}:{{line}}.',
    },
  },
  create(context) {
    const filename = context.filename ?? context.getFilename();
    if (!filename.endsWith(REPORT_FILE)) {
      return {};
    }

    const rootDir = dirname(dirname(dirname(filename)));

    return {
      "Program:exit"(node) {
        let candidates;
        try {
          candidates = collectPublicRustFunctions(rootDir);
        } catch (error) {
          context.report({
            node,
            messageId: "astGrepFailure",
            data: { error: error instanceof Error ? error.message : String(error) },
          });
          return;
        }

        const declarationCounts = new Map();
        for (const candidate of candidates) {
          declarationCounts.set(
            candidate.name,
            (declarationCounts.get(candidate.name) ?? 0) + 1,
          );
        }

        const allRustText = loadRustText(rootDir);
        for (const candidate of candidates) {
          if (candidate.file.endsWith("tauri_test_bridge.rs")) {
            continue;
          }

          const declarationCount = declarationCounts.get(candidate.name) ?? 1;
          if (countIdentifier(allRustText, candidate.name) > declarationCount) {
            continue;
          }

          context.report({
            node,
            messageId: "unusedPublicFunction",
            data: candidate,
          });
        }
      },
    };
  },
};
