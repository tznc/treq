import { readFileSync, readdirSync } from "node:fs";
import { dirname, join } from "node:path";

const API_FILES = [
  join("src", "lib", "api.ts"),
  join("src", "lib", "api-browser.ts"),
  join("src", "lib", "api-checks-logs.ts"),
  join("src", "lib", "api-extra.ts"),
  join("src", "lib", "api-github.ts"),
  join("src", "lib", "api-linear.ts"),
  join("src", "lib", "api-pr-status.ts"),
  join("src", "lib", "api-remote-ssh.ts"),
];
const API_REPORT_FILE = API_FILES[0];
const RUST_SRC_DIR = join("src-tauri", "src");
const COMMAND_REGEX =
  /#\[\s*tauri::command(?:\([^)]*\))?\s*]\s*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\s+([A-Za-z_]\w*)/gms;
const INVOKE_REGEX = /invoke\(\s*(?:"([^"]+)"|'([^']+)'|`([^`$]+)`)/g;

function collectRustFiles(dir, out = []) {
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

function collectRustCommandNames(rootDir) {
  const names = new Set();

  for (const file of collectRustFiles(join(rootDir, RUST_SRC_DIR))) {
    const text = readFileSync(file, "utf8");
    for (const match of text.matchAll(COMMAND_REGEX)) {
      names.add(match[1]);
    }
  }

  return names;
}

function collectApiCommandNames(rootDir) {
  const names = new Set();

  for (const relPath of API_FILES) {
    const text = readFileSync(join(rootDir, relPath), "utf8");
    for (const match of text.matchAll(INVOKE_REGEX)) {
      names.add(match[1] ?? match[2] ?? match[3]);
    }
  }

  return names;
}

export default {
  meta: {
    type: "problem",
    docs: {
      description:
        "Require every Rust Tauri command to have an invoke wrapper in src/lib/api.ts",
      recommended: false,
    },
    schema: [],
    messages: {
      missingWrapper:
        'Rust Tauri command "{{name}}" is missing an invoke wrapper in src/lib/api.ts.',
      noCommandsFound:
        "No Rust Tauri commands were discovered under src-tauri/src.",
    },
  },
  create(context) {
    const filename = context.filename ?? context.getFilename();
    if (!filename.endsWith(API_REPORT_FILE)) {
      return {};
    }

    const rootDir = dirname(dirname(dirname(filename)));
    const rustCommandNames = collectRustCommandNames(rootDir);
    const apiCommandNames = collectApiCommandNames(rootDir);

    return {
      "Program:exit"(node) {
        if (rustCommandNames.size === 0) {
          context.report({ node, messageId: "noCommandsFound" });
          return;
        }

        for (const name of [...rustCommandNames].sort()) {
          if (apiCommandNames.has(name)) {
            continue;
          }

          context.report({
            node,
            messageId: "missingWrapper",
            data: { name },
          });
        }
      },
    };
  },
};
