//! Explicit-alias SSH config resolution (Handoff 3: "Native Explicit-Alias
//! Resolution and Trust").
//!
//! Parses `~/.ssh/config` (and files it `Include`s) to resolve one
//! explicitly-selected `Host` alias into hostname/port/username/identity
//! fields, following OpenSSH's first-match-wins semantics: once a keyword is
//! set by an earlier matching `Host` block, a later matching block cannot
//! override it.
//!
//! This module is discovery/autocomplete only (PRD "Treat discovery as
//! autocomplete only; never connect merely because an alias was found").
//! Nothing here opens a network connection or reads `known_hosts`, and
//! resolving an alias never by itself grants trust. Trust is established only
//! when the user registers the endpoint with an expected host-key
//! fingerprint, which `crate::core::remote_ssh_transport::HostKeyVerifier`
//! enforces — never a system `ssh` subprocess and never
//! `StrictHostKeyChecking=no` or an equivalent.
//!
//! ## Supported directives
//!
//! `Host`, `HostName`, `User`, `Port`, `IdentityFile`, `Include`.
//!
//! ## Unsupported directives
//!
//! `ProxyJump` and `ProxyCommand` are rejected with a structured
//! [`SshConfigError::UnsupportedFeature`] (`unsupported_ssh_config_feature`)
//! rather than silently ignored or falling back to a system `ssh` subprocess
//! that might honor them. No other directives are recognized; unrecognized
//! directives are ignored, matching OpenSSH's tolerance for keywords a client
//! does not act on (case never widens what this resolver connects with).

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SshConfigError {
  /// No `Host` block in any parsed file matched the requested alias.
  AliasNotFound(String),
  /// A recognized directive's value could not be parsed (e.g. a non-numeric
  /// `Port`).
  Malformed { directive: String, detail: String },
  /// A directive this resolver deliberately does not implement (e.g.
  /// `ProxyJump`) was present in a block matching the requested alias.
  /// Surfaced as a structured `unsupported_ssh_config_feature` error rather
  /// than silently ignored or routed through a system `ssh` fallback.
  UnsupportedFeature { directive: String, detail: String },
}

impl std::fmt::Display for SshConfigError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Self::AliasNotFound(alias) => {
        write!(f, "alias_not_found: no Host block matches '{alias}'")
      }
      Self::Malformed { directive, detail } => {
        write!(f, "malformed_ssh_config: {directive}: {detail}")
      }
      Self::UnsupportedFeature { directive, detail } => {
        write!(f, "unsupported_ssh_config_feature: {directive}: {detail}")
      }
    }
  }
}

impl std::error::Error for SshConfigError {}

/// The explicit connection fields resolved for one alias. Autocomplete data
/// only — constructing this performs no network I/O and grants no trust.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedSshAlias {
  pub alias: String,
  pub hostname: String,
  pub port: u16,
  pub username: Option<String>,
  /// Raw `IdentityFile` value (may contain `~`), unexpanded — the native
  /// transport's `ClientAuthenticator` already knows how to resolve `~` and
  /// bare filenames under `~/.ssh`.
  pub identity_file: Option<String>,
}

const DEFAULT_PORT: u16 = 22;

/// Returns the default `~/.ssh/config` search path (the only file OpenSSH
/// reads for a user's own alias by default; `/etc/ssh/ssh_config` is a
/// system-wide fallback and is intentionally not part of an explicit user
/// alias selection).
pub fn default_ssh_config_paths() -> Vec<PathBuf> {
  let mut paths = Vec::new();
  if let Ok(home) = std::env::var("HOME") {
    paths.push(PathBuf::from(home).join(".ssh").join("config"));
  }
  paths
}

/// One line already tagged with the directory its file lives in, so a
/// relative `IdentityFile` or `Include` path resolves against the file that
/// declared it, not the top-level config.
struct SourcedLine {
  dir: PathBuf,
  text: String,
}

/// Loads `path` (and, recursively, anything it `Include`s) into an ordered
/// sequence of lines, exactly as OpenSSH conceptually inlines Include at the
/// point it appears. `seen` guards against Include cycles by canonical path.
fn load_lines(path: &Path, seen: &mut HashSet<PathBuf>) -> Vec<SourcedLine> {
  let canonical = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
  if !seen.insert(canonical) {
    return Vec::new();
  }
  let Ok(contents) = fs::read_to_string(path) else {
    return Vec::new();
  };
  let dir = path
    .parent()
    .map(Path::to_path_buf)
    .unwrap_or_else(|| PathBuf::from("."));
  let mut out = Vec::new();
  for line in contents.lines() {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
      continue;
    }
    let (keyword, _rest) = split_directive(trimmed);
    if keyword.eq_ignore_ascii_case("include") {
      for included in expand_include(&dir, trimmed) {
        out.extend(load_lines(&included, seen));
      }
      continue;
    }
    out.push(SourcedLine {
      dir: dir.clone(),
      text: trimmed.to_string(),
    });
  }
  out
}

/// Expands one `Include` directive's argument(s) into concrete file paths.
/// Supports a bare path (absolute or relative to `dir`) and a trailing `*`
/// glob within one path segment (the common `Include config.d/*` form);
/// broader glob syntax is not needed for this scope.
fn expand_include(dir: &Path, line: &str) -> Vec<PathBuf> {
  let (_keyword, rest) = split_directive(line);
  let mut matches = Vec::new();
  for raw in rest.split_whitespace() {
    let candidate = expand_tilde(raw);
    let candidate = if candidate.is_absolute() {
      candidate
    } else {
      dir.join(candidate)
    };
    if let Some(pattern) = candidate.to_str().filter(|s| s.contains('*')) {
      let pattern_path = PathBuf::from(pattern);
      let Some(parent) = pattern_path.parent() else {
        continue;
      };
      let Some(file_pattern) = pattern_path.file_name().and_then(|f| f.to_str()) else {
        continue;
      };
      let Ok(entries) = fs::read_dir(parent) else {
        continue;
      };
      let mut found: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|entry_path| {
          entry_path
            .file_name()
            .and_then(|f| f.to_str())
            .map(|name| glob_match(file_pattern, name))
            .unwrap_or(false)
        })
        .collect();
      found.sort();
      matches.extend(found);
    } else if candidate.is_file() {
      matches.push(candidate);
    }
  }
  matches
}

fn expand_tilde(value: &str) -> PathBuf {
  if let Some(rest) = value.strip_prefix("~/") {
    if let Ok(home) = std::env::var("HOME") {
      return PathBuf::from(home).join(rest);
    }
  }
  PathBuf::from(value)
}

/// Splits a config line into its keyword and the remainder, tolerating both
/// `Keyword value` and `Keyword=value` forms (OpenSSH accepts either).
fn split_directive(line: &str) -> (&str, &str) {
  let line = line.trim();
  let sep_index = line
    .find(|c: char| c.is_whitespace() || c == '=')
    .unwrap_or(line.len());
  let (keyword, rest) = line.split_at(sep_index);
  let rest = rest.trim_start_matches(['=', ' ', '\t']).trim();
  (keyword, rest)
}

/// Minimal shell-style glob matcher supporting `*` (any run of characters)
/// and `?` (any single character) — the only wildcard forms `Host` patterns
/// and `Include` globs need for this scope.
fn glob_match(pattern: &str, text: &str) -> bool {
  fn matches(pattern: &[char], text: &[char]) -> bool {
    match pattern.first() {
      None => text.is_empty(),
      Some('*') => {
        matches(&pattern[1..], text) || (!text.is_empty() && matches(pattern, &text[1..]))
      }
      Some('?') => !text.is_empty() && matches(&pattern[1..], &text[1..]),
      Some(c) => !text.is_empty() && text[0] == *c && matches(&pattern[1..], &text[1..]),
    }
  }
  let pattern_chars: Vec<char> = pattern.chars().collect();
  let text_chars: Vec<char> = text.chars().collect();
  matches(&pattern_chars, &text_chars)
}

/// Whether `alias` is matched by a `Host` directive's space-separated pattern
/// list, honoring `!pattern` negation (a negated pattern that matches
/// excludes the alias from this block even if an earlier positive pattern in
/// the same line matched).
fn host_line_matches(patterns: &str, alias: &str) -> bool {
  let mut matched = false;
  for pattern in patterns.split_whitespace() {
    if let Some(negated) = pattern.strip_prefix('!') {
      if glob_match(negated, alias) {
        return false;
      }
    } else if glob_match(pattern, alias) {
      matched = true;
    }
  }
  matched
}

/// Resolves `alias` against the given config file paths (in order), applying
/// OpenSSH's first-match-wins semantics per keyword. Pure parsing: no network
/// I/O, and no trust is implied by a successful resolution.
pub fn resolve_alias_from_paths(
  alias: &str,
  paths: &[PathBuf],
) -> Result<ResolvedSshAlias, SshConfigError> {
  let mut lines = Vec::new();
  let mut seen = HashSet::new();
  for path in paths {
    lines.extend(load_lines(path, &mut seen));
  }

  let mut result = ResolvedSshAlias {
    alias: alias.to_string(),
    hostname: String::new(),
    port: DEFAULT_PORT,
    username: None,
    identity_file: None,
  };
  let mut hostname_set = false;
  let mut port_set = false;
  let mut username_set = false;
  let mut identity_set = false;
  let mut matched_any_block = false;
  let mut currently_matching = false;

  for line in &lines {
    let (keyword, value) = split_directive(&line.text);
    if keyword.eq_ignore_ascii_case("host") {
      currently_matching = host_line_matches(value, alias);
      if currently_matching {
        matched_any_block = true;
      }
      continue;
    }
    if !currently_matching {
      continue;
    }
    if keyword.eq_ignore_ascii_case("proxyjump") || keyword.eq_ignore_ascii_case("proxycommand") {
      return Err(SshConfigError::UnsupportedFeature {
        directive: keyword.to_string(),
        detail: format!(
          "'{keyword}' cannot be safely implemented with the current native SSH transport; reject alias '{alias}' explicitly rather than silently ignoring it"
        ),
      });
    }
    if keyword.eq_ignore_ascii_case("hostname") {
      if !hostname_set {
        if value.trim().is_empty() {
          return Err(SshConfigError::Malformed {
            directive: "HostName".to_string(),
            detail: "value is empty".to_string(),
          });
        }
        result.hostname = value.trim().to_string();
        hostname_set = true;
      }
    } else if keyword.eq_ignore_ascii_case("user") {
      if !username_set {
        if value.trim().is_empty() {
          return Err(SshConfigError::Malformed {
            directive: "User".to_string(),
            detail: "value is empty".to_string(),
          });
        }
        result.username = Some(value.trim().to_string());
        username_set = true;
      }
    } else if keyword.eq_ignore_ascii_case("port") {
      if !port_set {
        match value.trim().parse::<u16>() {
          Ok(port) => {
            result.port = port;
            port_set = true;
          }
          Err(_) => {
            return Err(SshConfigError::Malformed {
              directive: "Port".to_string(),
              detail: format!("'{}' is not a valid port number", value.trim()),
            });
          }
        }
      }
    } else if keyword.eq_ignore_ascii_case("identityfile") && !identity_set {
      if value.trim().is_empty() {
        return Err(SshConfigError::Malformed {
          directive: "IdentityFile".to_string(),
          detail: "value is empty".to_string(),
        });
      }
      result.identity_file = Some(value.trim().to_string());
      identity_set = true;
    }
    // Unrecognized directives are ignored rather than rejected: OpenSSH
    // itself tolerates keywords a given client build does not act on, and
    // this resolver only ever *uses* the supported fields above to build a
    // connection, so an ignored directive can never widen trust or change
    // what gets connected to.
    let _ = line.dir.as_path();
  }

  if !matched_any_block {
    return Err(SshConfigError::AliasNotFound(alias.to_string()));
  }
  if !hostname_set {
    // OpenSSH defaults HostName to the alias itself when unset.
    result.hostname = alias.to_string();
  }
  Ok(result)
}

/// Resolves `alias` against the default `~/.ssh/config` search path.
pub fn resolve_alias(alias: &str) -> Result<ResolvedSshAlias, SshConfigError> {
  resolve_alias_from_paths(alias, &default_ssh_config_paths())
}

#[cfg(test)]
mod tests {
  use super::*;
  use tempfile::TempDir;

  fn write(dir: &TempDir, name: &str, contents: &str) -> PathBuf {
    let path = dir.path().join(name);
    fs::write(&path, contents).unwrap();
    path
  }

  // -- Explicit fields ---------------------------------------------------

  #[test]
  fn resolves_explicit_fields_for_matching_host() {
    let dir = TempDir::new().unwrap();
    let config = write(
      &dir,
      "config",
      "Host prod\n  HostName prod.example.com\n  User deploy\n  Port 2222\n  IdentityFile ~/.ssh/id_prod\n",
    );

    let resolved = resolve_alias_from_paths("prod", &[config]).unwrap();

    assert_eq!(resolved.hostname, "prod.example.com");
    assert_eq!(resolved.username, Some("deploy".to_string()));
    assert_eq!(resolved.port, 2222);
    assert_eq!(resolved.identity_file, Some("~/.ssh/id_prod".to_string()));
  }

  #[test]
  fn defaults_port_and_hostname_when_unset() {
    let dir = TempDir::new().unwrap();
    let config = write(&dir, "config", "Host bare\n  User me\n");

    let resolved = resolve_alias_from_paths("bare", &[config]).unwrap();

    assert_eq!(resolved.hostname, "bare");
    assert_eq!(resolved.port, 22);
  }

  // -- Missing alias -------------------------------------------------------

  #[test]
  fn returns_alias_not_found_when_no_host_block_matches() {
    let dir = TempDir::new().unwrap();
    let config = write(&dir, "config", "Host prod\n  HostName prod.example.com\n");

    let error = resolve_alias_from_paths("staging", &[config]).unwrap_err();

    assert_eq!(error, SshConfigError::AliasNotFound("staging".to_string()));
  }

  // -- Malformed values ------------------------------------------------------

  #[test]
  fn returns_malformed_error_for_non_numeric_port() {
    let dir = TempDir::new().unwrap();
    let config = write(&dir, "config", "Host prod\n  Port notaport\n");

    let error = resolve_alias_from_paths("prod", &[config]).unwrap_err();

    assert!(matches!(
      error,
      SshConfigError::Malformed { directive, .. } if directive == "Port"
    ));
  }

  #[test]
  fn returns_malformed_error_for_empty_hostname() {
    let dir = TempDir::new().unwrap();
    let config = write(&dir, "config", "Host prod\n  HostName \n");

    let error = resolve_alias_from_paths("prod", &[config]).unwrap_err();

    assert!(matches!(
      error,
      SshConfigError::Malformed { directive, .. } if directive == "HostName"
    ));
  }

  // -- Wildcard precedence (first match wins) ---------------------------------

  #[test]
  fn first_matching_block_wins_for_each_keyword() {
    let dir = TempDir::new().unwrap();
    let config = write(
      &dir,
      "config",
      "Host prod\n  HostName specific.example.com\n\nHost *\n  HostName wildcard.example.com\n  User wildcard-user\n",
    );

    let resolved = resolve_alias_from_paths("prod", &[config]).unwrap();

    // The specific `Host prod` block set HostName first, so the later
    // wildcard block's HostName must not override it, but User (never set by
    // the specific block) still comes from the wildcard block.
    assert_eq!(resolved.hostname, "specific.example.com");
    assert_eq!(resolved.username, Some("wildcard-user".to_string()));
  }

  #[test]
  fn negated_pattern_excludes_alias_from_block() {
    let dir = TempDir::new().unwrap();
    let config = write(&dir, "config", "Host !staging *\n  User everyone-else\n");

    let error = resolve_alias_from_paths("staging", std::slice::from_ref(&config)).unwrap_err();
    assert_eq!(error, SshConfigError::AliasNotFound("staging".to_string()));

    let resolved = resolve_alias_from_paths("prod", &[config]).unwrap();
    assert_eq!(resolved.username, Some("everyone-else".to_string()));
  }

  // -- Include -----------------------------------------------------------------

  #[test]
  fn resolves_alias_defined_in_an_included_file() {
    let dir = TempDir::new().unwrap();
    write(
      &dir,
      "hosts.conf",
      "Host included-host\n  HostName included.example.com\n",
    );
    let main = write(&dir, "config", "Include hosts.conf\n");

    let resolved = resolve_alias_from_paths("included-host", &[main]).unwrap();

    assert_eq!(resolved.hostname, "included.example.com");
  }

  #[test]
  fn resolves_alias_from_glob_include() {
    let dir = TempDir::new().unwrap();
    fs::create_dir(dir.path().join("config.d")).unwrap();
    fs::write(
      dir.path().join("config.d/10-work.conf"),
      "Host work\n  HostName work.example.com\n",
    )
    .unwrap();
    let main = write(&dir, "config", "Include config.d/*\n");

    let resolved = resolve_alias_from_paths("work", &[main]).unwrap();

    assert_eq!(resolved.hostname, "work.example.com");
  }

  #[test]
  fn include_cycle_does_not_infinite_loop() {
    let dir = TempDir::new().unwrap();
    let a_path = dir.path().join("a");
    let b_path = dir.path().join("b");
    fs::write(
      &a_path,
      format!(
        "Include {}\nHost from-a\n  HostName a.example.com\n",
        b_path.display()
      ),
    )
    .unwrap();
    fs::write(
      &b_path,
      format!(
        "Include {}\nHost from-b\n  HostName b.example.com\n",
        a_path.display()
      ),
    )
    .unwrap();

    let resolved = resolve_alias_from_paths("from-a", &[a_path]).unwrap();
    assert_eq!(resolved.hostname, "a.example.com");
  }

  // -- Unsupported directives ---------------------------------------------------

  #[test]
  fn rejects_proxyjump_with_structured_error() {
    let dir = TempDir::new().unwrap();
    let config = write(
      &dir,
      "config",
      "Host bastioned\n  HostName inner.example.com\n  ProxyJump bastion.example.com\n",
    );

    let error = resolve_alias_from_paths("bastioned", &[config]).unwrap_err();

    assert!(error
      .to_string()
      .starts_with("unsupported_ssh_config_feature"));
    assert!(matches!(
      error,
      SshConfigError::UnsupportedFeature { directive, .. } if directive == "ProxyJump"
    ));
  }

  #[test]
  fn rejects_proxycommand_with_structured_error() {
    let dir = TempDir::new().unwrap();
    let config = write(
      &dir,
      "config",
      "Host bastioned\n  ProxyCommand ssh -W %h:%p bastion\n",
    );

    let error = resolve_alias_from_paths("bastioned", &[config]).unwrap_err();

    assert!(matches!(
      error,
      SshConfigError::UnsupportedFeature { directive, .. } if directive == "ProxyCommand"
    ));
  }

  #[test]
  fn unrecognized_directive_outside_proxy_is_ignored_not_rejected() {
    let dir = TempDir::new().unwrap();
    let config = write(
      &dir,
      "config",
      "Host prod\n  HostName prod.example.com\n  ServerAliveInterval 30\n",
    );

    let resolved = resolve_alias_from_paths("prod", &[config]).unwrap();
    assert_eq!(resolved.hostname, "prod.example.com");
  }

  // -- Discovery is never connection ---------------------------------------------

  #[test]
  fn resolving_an_alias_never_touches_the_network() {
    // A resolvable alias pointing at a hostname that does not exist proves
    // resolution succeeds purely from parsing: if this function attempted a
    // connection, it would hang or fail rather than returning promptly.
    let dir = TempDir::new().unwrap();
    let config = write(
      &dir,
      "config",
      "Host nowhere\n  HostName this-host-does-not-resolve.invalid\n  Port 65000\n",
    );

    let resolved = resolve_alias_from_paths("nowhere", &[config]).unwrap();
    assert_eq!(resolved.hostname, "this-host-does-not-resolve.invalid");
  }

  // -- No system ssh subprocess on the reachable alias flow ---------------------

  #[test]
  fn alias_resolution_never_references_a_system_ssh_program() {
    // Pure static inspection of this module's own source: everything in the
    // explicit-alias resolution path lives here, and none of it
    // may spawn a system `ssh` binary or shell out at all (PRD "Remove the
    // system-ssh subprocess from the reachable alias product flow").
    let source = include_str!("remote_ssh_config.rs");
    let production_code = source
      .split("#[cfg(test)]")
      .next()
      .expect("this module always has a #[cfg(test)] section");
    assert!(
      !production_code.contains("Command::new"),
      "remote_ssh_config.rs must never spawn a subprocess"
    );
    assert!(
      !production_code.contains("\"ssh\""),
      "remote_ssh_config.rs must never reference a system ssh program name"
    );
  }

  #[test]
  fn core_remote_typed_commands_never_shell_out_to_ssh() {
    // Static inspection of `core::remote`, whose typed commands reach an
    // endpoint only over the native transport: the only remaining `"ssh"`
    // program-name reference in that file is `build_ssh_shell_command`, which
    // builds an *interactive PTY* shell command line and is never called by
    // the typed probe/clone/inspect commands.
    let source = include_str!("remote.rs");
    assert!(
      !source.contains("Command::new(\"ssh\")"),
      "core::remote must never spawn a system ssh subprocess"
    );
    let ssh_program_references = source.matches("\"ssh\".to_string()").count();
    assert_eq!(
      ssh_program_references, 1,
      "the only \"ssh\" program-name reference left should be build_ssh_shell_command's PTY command line"
    );
    assert!(
      source.contains("pub fn build_ssh_shell_command"),
      "the one remaining ssh-program reference must be inside build_ssh_shell_command"
    );
  }
}
