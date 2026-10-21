//! Which changed files are generated, as GitHub decides it: the
//! `linguist-generated` attribute in `.gitattributes`, else Linguist's
//! well-known generated paths (lockfiles, minified bundles, source maps,
//! protobuf output). Their diffs wait behind "Load diff".

use std::path::Path;

use ignore::gitignore::{Gitignore, GitignoreBuilder};

/// Lockfiles and other files tools write, by name.
const GENERATED_NAMES: &[&str] = &[
  "package-lock.json",
  "npm-shrinkwrap.json",
  "yarn.lock",
  "pnpm-lock.yaml",
  "bun.lock",
  "bun.lockb",
  "deno.lock",
  "Cargo.lock",
  "Gemfile.lock",
  "composer.lock",
  "poetry.lock",
  "Pipfile.lock",
  "pdm.lock",
  "uv.lock",
  "pixi.lock",
  "go.sum",
  "Gopkg.lock",
  "glide.lock",
  "flake.lock",
  "pubspec.lock",
  "Package.resolved",
  "Podfile.lock",
  "mix.lock",
  "MODULE.bazel.lock",
  "packages.lock.json",
  "paket.lock",
  ".pnp.js",
  ".pnp.cjs",
  ".pnp.loader.mjs",
];

/// Minified bundles, source maps and code generators' output, by suffix.
const GENERATED_SUFFIXES: &[&str] = &[
  ".min.js",
  ".min.mjs",
  ".min.css",
  ".js.map",
  ".mjs.map",
  ".css.map",
  ".pb.go",
  ".pb.cc",
  ".pb.h",
  ".pb.swift",
  "_pb2.py",
  "_pb2_grpc.py",
  "_pb2.pyi",
  "_pb.js",
  "_pb.d.ts",
  "_grpc_pb.js",
  ".designer.cs",
  ".g.dart",
  ".freezed.dart",
  ".xcworkspacedata",
  ".xcuserstate",
];

/// Directories whose files are all generated.
const GENERATED_DIRS: &[&str] = &["node_modules", "__generated__"];

/// One `.gitattributes` line that sets or unsets `linguist-generated`.
struct Rule {
  pattern: Gitignore,
  /// `None` for `!linguist-generated`: back to the built-in paths.
  generated: Option<bool>,
}

/// Decides which paths are generated. `default()` knows only the built-in
/// paths.
#[derive(Default)]
pub struct GeneratedFiles {
  rules: Vec<Rule>,
}

impl GeneratedFiles {
  /// The `linguist-generated` lines of a root `.gitattributes`.
  pub fn parse(gitattributes: &str) -> Self {
    let rules = gitattributes
      .lines()
      .filter_map(|line| {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
          return None;
        }
        let mut parts = line.split_whitespace();
        let pattern = parts.next()?;
        // Negative patterns are not allowed in `.gitattributes`.
        if pattern.starts_with('!') {
          return None;
        }
        let generated = parts.filter_map(attribute).next_back()?;
        let mut builder = GitignoreBuilder::new("");
        builder.add_line(None, pattern).ok()?;
        Some(Rule {
          pattern: builder.build().ok()?,
          generated,
        })
      })
      .collect();
    Self { rules }
  }

  /// Reads `.gitattributes` at the root of `dir`; none there is no rules.
  pub fn read(dir: &str) -> Self {
    std::fs::read_to_string(Path::new(dir).join(".gitattributes"))
      .map(|text| Self::parse(&text))
      .unwrap_or_default()
  }

  /// The last `.gitattributes` line matching `path` decides, else the
  /// built-in paths do.
  pub fn is_generated(&self, path: &str) -> bool {
    self
      .rules
      .iter()
      .rev()
      .find(|rule| rule.pattern.matched(path, false).is_ignore())
      .and_then(|rule| rule.generated)
      .unwrap_or_else(|| is_known_generated(path))
  }
}

/// What a `.gitattributes` token says of `linguist-generated`, if it names it:
/// `Some(None)` for an unset (`!`) attribute.
fn attribute(token: &str) -> Option<Option<bool>> {
  match token {
    "linguist-generated" | "linguist-generated=true" => Some(Some(true)),
    "-linguist-generated" | "linguist-generated=false" => Some(Some(false)),
    "!linguist-generated" => Some(None),
    _ => None,
  }
}

/// Linguist's well-known generated paths.
pub fn is_known_generated(path: &str) -> bool {
  let name = path.rsplit('/').next().unwrap_or(path);
  GENERATED_NAMES.contains(&name)
    || GENERATED_SUFFIXES.iter().any(|s| name.ends_with(s))
    || path
      .split('/')
      .rev()
      .skip(1)
      .any(|dir| GENERATED_DIRS.contains(&dir))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn treats_lockfiles_as_generated() {
    let files = GeneratedFiles::default();
    assert!(files.is_generated("Cargo.lock"));
    assert!(files.is_generated("web/package-lock.json"));
    assert!(files.is_generated("go.sum"));
    assert!(!files.is_generated("src/lock.rs"));
  }

  #[test]
  fn treats_minified_and_protobuf_output_as_generated() {
    let files = GeneratedFiles::default();
    assert!(files.is_generated("dist/app.min.js"));
    assert!(files.is_generated("dist/app.js.map"));
    assert!(files.is_generated("api/service.pb.go"));
    assert!(files.is_generated("api/service_pb2.py"));
    assert!(!files.is_generated("src/app.js"));
  }

  #[test]
  fn treats_files_under_generated_dirs_as_generated() {
    let files = GeneratedFiles::default();
    assert!(files.is_generated("web/node_modules/a/index.js"));
    assert!(files.is_generated("src/__generated__/schema.ts"));
    assert!(!files.is_generated("node_modules.md"));
  }

  #[test]
  fn marks_paths_generated_from_gitattributes() {
    let files = GeneratedFiles::parse(
      "# api\nsrc/gen/** linguist-generated\n*.snap linguist-generated=true\n",
    );
    assert!(files.is_generated("src/gen/client.ts"));
    assert!(files.is_generated("tests/ui.snap"));
    assert!(!files.is_generated("src/main.ts"));
  }

  #[test]
  fn unmarks_known_generated_paths_from_gitattributes() {
    let files =
      GeneratedFiles::parse("Cargo.lock -linguist-generated\nyarn.lock linguist-generated=false\n");
    assert!(!files.is_generated("Cargo.lock"));
    assert!(!files.is_generated("yarn.lock"));
    assert!(files.is_generated("go.sum"));
  }

  #[test]
  fn last_matching_gitattributes_line_wins() {
    let files = GeneratedFiles::parse(
      "gen/** linguist-generated\ngen/keep.ts -linguist-generated\nCargo.lock -linguist-generated\nCargo.lock !linguist-generated\n",
    );
    assert!(files.is_generated("gen/a.ts"));
    assert!(!files.is_generated("gen/keep.ts"));
    assert!(
      files.is_generated("Cargo.lock"),
      "unset falls back to the built-in paths"
    );
  }

  #[test]
  fn ignores_lines_without_linguist_generated() {
    let files = GeneratedFiles::parse("* text=auto\n*.png binary\n");
    assert!(!files.is_generated("a.png"));
    assert!(files.is_generated("Cargo.lock"));
  }

  #[test]
  fn reads_gitattributes_from_the_workspace_root() {
    let dir = tempfile::TempDir::new().unwrap();
    std::fs::write(
      dir.path().join(".gitattributes"),
      "*.gen.ts linguist-generated\n",
    )
    .unwrap();
    let files = GeneratedFiles::read(dir.path().to_str().unwrap());
    assert!(files.is_generated("src/a.gen.ts"));
  }

  #[test]
  fn reads_no_rules_without_gitattributes() {
    let dir = tempfile::TempDir::new().unwrap();
    let files = GeneratedFiles::read(dir.path().to_str().unwrap());
    assert!(!files.is_generated("src/a.gen.ts"));
    assert!(files.is_generated("Cargo.lock"));
  }
}
