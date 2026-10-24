//! Shared plumbing for issue-tracker integrations: branch naming and
//! per-repo settings.

/// Lowercase, hyphen-separated slug of `title`, capped at `max_len` bytes on
/// a word boundary where possible. Returns an empty string for titles with no
/// ASCII alphanumerics.
#[allow(
  clippy::string_slice,
  reason = "the slug holds only ASCII alphanumerics and dashes"
)]
pub fn branch_slug(title: &str, max_len: usize) -> String {
  let mut slug = String::new();
  let mut pending_dash = false;
  for ch in title.chars() {
    if ch.is_ascii_alphanumeric() {
      if pending_dash && !slug.is_empty() {
        slug.push('-');
      }
      pending_dash = false;
      slug.push(ch.to_ascii_lowercase());
    } else {
      pending_dash = true;
    }
  }
  if slug.len() <= max_len {
    return slug;
  }
  let cut = &slug[..max_len];
  match cut.rfind('-') {
    Some(idx) if idx > 0 => cut[..idx].to_string(),
    _ => cut.trim_end_matches('-').to_string(),
  }
}

/// Joins a branch prefix and title slug, dropping the slug when it is empty.
pub fn branch_name(prefix: &str, title: &str) -> String {
  let slug = branch_slug(title, 48);
  if slug.is_empty() {
    prefix.to_string()
  } else {
    format!("{prefix}-{slug}")
  }
}

/// Reads a non-empty per-repo setting, trimmed.
pub fn read_setting(
  db: &crate::db::Database,
  repo_path: &str,
  key: &str,
) -> Result<Option<String>, String> {
  let value = db
    .get_repo_setting(repo_path, key)
    .map_err(|e| format!("Failed to read {key}: {e}"))?;
  Ok(
    value
      .map(|v| v.trim().to_string())
      .filter(|v| !v.is_empty()),
  )
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn branch_slug_collapses_punctuation() {
    assert_eq!(
      branch_slug("Fix: login  bug (iOS)!", 48),
      "fix-login-bug-ios"
    );
  }

  #[test]
  fn branch_slug_truncates_on_word_boundary() {
    assert_eq!(branch_slug("alpha beta gamma", 12), "alpha-beta");
  }

  #[test]
  fn branch_slug_drops_non_ascii_only_titles() {
    assert_eq!(branch_slug("修正する", 48), "");
    assert_eq!(branch_name("ENG-1", "修正する"), "ENG-1");
  }

  #[test]
  fn branch_name_joins_prefix_and_slug() {
    assert_eq!(
      branch_name("ENG-12", "Add login support"),
      "ENG-12-add-login-support"
    );
  }
}
