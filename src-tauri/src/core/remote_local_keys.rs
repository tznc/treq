//! Enumerates the user's existing local SSH public key identities for the
//! Phase 6 remote-setup UI (prds/remote-development.md, "Treq-managed VM": "The user
//! selects: ... an existing local public/private key identity"; "Treq does
//! not create a private key").
//!
//! This only ever reads `*.pub` files under `~/.ssh` - a public key and its
//! comment - and computes the SHA256 fingerprint the UI shows before
//! registration. It never reads, generates, or transmits a private key.

use russh::keys::ssh_key::PublicKey;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalSshIdentity {
  /// Stable reference the rest of the app uses to mean "this local key" -
  /// currently its path, since that is what a later signing/auth step needs
  /// to locate the matching private half.
  pub reference: String,
  pub label: String,
  pub fingerprint_sha256: String,
  pub algorithm: String,
}

fn ssh_dir() -> Option<PathBuf> {
  std::env::var("HOME")
    .ok()
    .map(|home| PathBuf::from(home).join(".ssh"))
}

pub fn list_local_ssh_identities() -> Result<Vec<LocalSshIdentity>, String> {
  let Some(dir) = ssh_dir() else {
    return Ok(Vec::new());
  };
  let Ok(entries) = std::fs::read_dir(&dir) else {
    return Ok(Vec::new());
  };

  let mut identities = Vec::new();
  for entry in entries.flatten() {
    let path = entry.path();
    if path.extension().and_then(|ext| ext.to_str()) != Some("pub") {
      continue;
    }
    let Ok(contents) = std::fs::read_to_string(&path) else {
      continue;
    };
    let Ok(key) = PublicKey::from_openssh(contents.trim()) else {
      continue;
    };
    let fingerprint = key.fingerprint(russh::keys::HashAlg::Sha256).to_string();
    let label = path
      .file_stem()
      .and_then(|s| s.to_str())
      .unwrap_or("id")
      .to_string();
    identities.push(LocalSshIdentity {
      reference: path.to_string_lossy().to_string(),
      label,
      fingerprint_sha256: fingerprint,
      algorithm: key.algorithm().to_string(),
    });
  }
  identities.sort_by(|a, b| a.label.cmp(&b.label));
  Ok(identities)
}

/// Reads the raw OpenSSH public-key text for a previously listed identity,
/// so the managed-VM setup flow can register it with the control plane
/// (prds/remote-development.md, "Client key policy": "Users select an existing key
/// identity ... Private keys remain on the user's device").
///
/// `reference` must be exactly a `reference` value returned by
/// [`list_local_ssh_identities`] (an absolute path to a `*.pub` file under
/// `~/.ssh`). This is deliberately strict - it never reads an arbitrary path
/// the frontend might pass, and it never reads a private-key file (which
/// would not end in `.pub` and would not be returned by `list_local_ssh_identities`
/// in the first place).
pub fn read_local_public_key(reference: &str) -> Result<String, String> {
  let Some(dir) = ssh_dir() else {
    return Err("No local SSH identity found (HOME is not set).".to_string());
  };
  let path = PathBuf::from(reference);
  if path.extension().and_then(|ext| ext.to_str()) != Some("pub") {
    return Err("Not a public key reference".to_string());
  }
  let canonical_dir = std::fs::canonicalize(&dir)
    .map_err(|_| "Local SSH identity directory is unavailable".to_string())?;
  let canonical_path =
    std::fs::canonicalize(&path).map_err(|_| "Local SSH identity no longer exists".to_string())?;
  if !canonical_path.starts_with(&canonical_dir) {
    return Err("Local SSH identity reference is outside ~/.ssh".to_string());
  }
  let contents = std::fs::read_to_string(&canonical_path)
    .map_err(|e| format!("Failed to read local SSH identity: {e}"))?;
  // Validate it really is a well-formed public key rather than returning
  // arbitrary file contents.
  PublicKey::from_openssh(contents.trim())
    .map_err(|e| format!("Not a valid OpenSSH public key: {e}"))?;
  Ok(contents.trim().to_string())
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::io::Write;
  use tempfile::TempDir;

  #[test]
  fn returns_empty_when_ssh_dir_missing() {
    // Smoke test only: exercising the real ~/.ssh in CI would be
    // environment-dependent, so this just asserts the function does not
    // panic and returns a list (possibly empty) rather than erroring.
    let result = list_local_ssh_identities();
    result.unwrap();
  }

  fn with_fake_home<F: FnOnce(&std::path::Path)>(f: F) {
    let tmp = TempDir::new().unwrap();
    let ssh = tmp.path().join(".ssh");
    std::fs::create_dir_all(&ssh).unwrap();
    let prev = std::env::var("HOME").ok();
    std::env::set_var("HOME", tmp.path());
    f(&ssh);
    match prev {
      Some(v) => std::env::set_var("HOME", v),
      None => std::env::remove_var("HOME"),
    }
  }

  const TEST_PUBKEY: &str =
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIBWtypxSPtLuNKcOKQ+z0nJXYSjbYyBSCzqDG3TAWZ4z test@example";

  #[test]
  fn reads_public_key_contents_for_a_listed_identity() {
    with_fake_home(|ssh| {
      let path = ssh.join("id_ed25519.pub");
      let mut file = std::fs::File::create(&path).unwrap();
      writeln!(file, "{TEST_PUBKEY}").unwrap();

      let reference = path.to_string_lossy().to_string();
      let contents = read_local_public_key(&reference).unwrap();
      assert_eq!(contents, TEST_PUBKEY);
    });
  }

  #[test]
  fn rejects_a_reference_outside_the_ssh_directory() {
    with_fake_home(|_ssh| {
      let outside = TempDir::new().unwrap();
      let path = outside.path().join("evil.pub");
      let mut file = std::fs::File::create(&path).unwrap();
      writeln!(file, "{TEST_PUBKEY}").unwrap();

      let reference = path.to_string_lossy().to_string();
      let error = read_local_public_key(&reference).unwrap_err();
      assert!(error.contains("outside"));
    });
  }

  #[test]
  fn rejects_a_non_pub_reference() {
    with_fake_home(|ssh| {
      let path = ssh.join("id_ed25519");
      std::fs::write(&path, "not actually a private key").unwrap();
      let reference = path.to_string_lossy().to_string();
      let error = read_local_public_key(&reference).unwrap_err();
      assert_eq!(error, "Not a public key reference");
    });
  }
}
