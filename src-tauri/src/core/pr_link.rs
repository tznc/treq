//! Links to a pull request in Treq: `treq://pr/open?repo={owner}/{repo}&number={n}`.
//! treq.dev/open/pr/{owner}/{repo}/{n} redirects to it in the browser.

/// The PR a `treq://pr/open` link names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrLink {
  /// GitHub `owner/repo`.
  pub repo: String,
  pub number: u64,
}

/// The PR a `treq://pr/open` link names, or `None` for any other link.
pub fn parse_pr_open_link(url: &str) -> Option<PrLink> {
  let parsed = url::Url::parse(url).ok()?;
  if parsed.scheme() != "treq" || parsed.host_str() != Some("pr") || parsed.path() != "/open" {
    return None;
  }
  let param = |name: &str| {
    parsed
      .query_pairs()
      .find(|(key, _)| key == name)
      .map(|(_, value)| value.into_owned())
  };
  let repo = param("repo")?;
  let (owner, name) = repo.split_once('/')?;
  if !is_github_name(owner) || !is_github_name(name) {
    return None;
  }
  let number = param("number")?.parse::<u64>().ok().filter(|n| *n > 0)?;
  Some(PrLink { repo, number })
}

static PENDING: std::sync::Mutex<Option<PrLink>> = std::sync::Mutex::new(None);

/// Keeps a `treq://pr/open` link that launched Treq, before any window has
/// a repository open.
pub fn queue_pending(link: PrLink) {
  *PENDING.lock().unwrap_or_else(|p| p.into_inner()) = Some(link);
}

/// The queued link, once, if it names GitHub repository `repo`.
pub fn take_pending_for(repo: &str) -> Option<PrLink> {
  let mut pending = PENDING.lock().unwrap_or_else(|p| p.into_inner());
  if pending.as_ref()?.repo.eq_ignore_ascii_case(repo) {
    pending.take()
  } else {
    None
  }
}

/// Whether a link is queued.
pub fn has_pending() -> bool {
  PENDING.lock().unwrap_or_else(|p| p.into_inner()).is_some()
}

/// The queued link, once, whatever its repository.
pub fn take_pending() -> Option<PrLink> {
  PENDING.lock().unwrap_or_else(|p| p.into_inner()).take()
}

/// A GitHub owner or repository name: letters, digits, `-`, `_` and `.`,
/// never `.` or `..` alone.
fn is_github_name(part: &str) -> bool {
  !part.is_empty()
    && part != "."
    && part != ".."
    && part
      .chars()
      .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn a_queued_link_is_taken_once_by_its_repo() {
    let link = PrLink {
      repo: "tznc/treq".into(),
      number: 42,
    };
    queue_pending(link.clone());
    assert!(has_pending());
    assert_eq!(take_pending_for("tznc/other"), None);
    assert_eq!(take_pending_for("TZNC/treq"), Some(link.clone()));
    assert_eq!(take_pending_for("tznc/treq"), None);
    queue_pending(link.clone());
    assert_eq!(take_pending(), Some(link));
    assert_eq!(take_pending(), None);
    assert!(!has_pending());
  }

  #[test]
  fn reads_the_repo_and_number() {
    assert_eq!(
      parse_pr_open_link("treq://pr/open?repo=tznc%2Ftreq&number=42"),
      Some(PrLink {
        repo: "tznc/treq".into(),
        number: 42
      })
    );
  }

  #[test]
  fn rejects_other_routes_and_bad_values() {
    for url in [
      "treq://pr/open",
      "treq://pr/open?repo=tznc%2Ftreq",
      "treq://pr/open?number=42",
      "treq://pr/open?repo=tznc&number=42",
      "treq://pr/open?repo=tznc%2Ftreq%2Fx&number=42",
      "treq://pr/open?repo=..%2Ftreq&number=42",
      "treq://pr/open?repo=a%20b%2Ftreq&number=42",
      "treq://pr/open?repo=tznc%2Ftreq&number=0",
      "treq://pr/open?repo=tznc%2Ftreq&number=x",
      "treq://pr/close?repo=tznc%2Ftreq&number=42",
      "https://treq.dev/pr/open?repo=tznc%2Ftreq&number=42",
    ] {
      assert_eq!(parse_pr_open_link(url), None, "{url}");
    }
  }
}
