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

/// The shareable treq.dev link for GitHub PR URL `github_url`
/// (`https://github.com/{owner}/{repo}/pull/{n}`), or `None` for any other URL.
pub fn treq_web_link(github_url: &str) -> Option<String> {
  let path = github_url.strip_prefix("https://github.com/")?;
  let mut parts = path.split('/');
  let (owner, repo, pull, number) = (parts.next()?, parts.next()?, parts.next()?, parts.next()?);
  if parts.next().is_some() || pull != "pull" || !is_github_name(owner) || !is_github_name(repo) {
    return None;
  }
  let number = number.parse::<u64>().ok().filter(|n| *n > 0)?;
  Some(format!("https://treq.dev/open/pr/{owner}/{repo}/{number}"))
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
  fn shares_a_github_pr_url_as_a_treq_dev_link() {
    assert_eq!(
      treq_web_link("https://github.com/tznc/treq/pull/42").as_deref(),
      Some("https://treq.dev/open/pr/tznc/treq/42")
    );
    for url in [
      "https://github.com/tznc/treq/pull/42/checks",
      "https://github.com/tznc/treq/issues/42",
      "https://github.com/tznc/treq",
      "https://linear.app/t/ENG-12",
    ] {
      assert_eq!(treq_web_link(url), None, "{url}");
    }
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
