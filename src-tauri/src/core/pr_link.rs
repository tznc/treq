//! Links to a pull request in Treq: `treq://pr/open?repo={owner}/{repo}&number={n}`.
//! treq.dev/open/pr/{owner}/{repo}/{n} redirects to it in the browser.
//! `treq://repo/open?repo={owner}/{repo}` shows the repository's window, and
//! `treq://agent/kickoff?repo={owner}/{repo}&branch={b}&prompt={p}` starts an
//! agent in it: the links a browser can make, knowing only the GitHub names.

/// The PR a `treq://pr/open` link names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrLink {
  /// GitHub `owner/repo`.
  pub repo: String,
  pub number: u64,
}

/// The PR a `treq://pr/open` link names, or `None` for any other link.
pub fn parse_pr_open_link(url: &str) -> Option<PrLink> {
  let parsed = treq_link(url, "pr", "/open")?;
  let repo = github_repo(&parsed)?;
  let number = param(&parsed, "number")?
    .parse::<u64>()
    .ok()
    .filter(|n| *n > 0)?;
  Some(PrLink { repo, number })
}

/// The GitHub `owner/repo` a `treq://repo/open` link names, or `None` for
/// any other link.
pub fn parse_repo_open_link(url: &str) -> Option<String> {
  github_repo(&treq_link(url, "repo", "/open")?)
}

/// What a `treq://agent/kickoff` link asks: an agent with `prompt` on
/// `branch` of GitHub repository `repo`, wherever a window has it open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KickoffLink {
  pub repo: String,
  pub branch: String,
  pub prompt: String,
}

/// The agent a `treq://agent/kickoff` link asks for, or `None` for any other
/// link.
pub fn parse_agent_kickoff_link(url: &str) -> Option<KickoffLink> {
  let parsed = treq_link(url, "agent", "/kickoff")?;
  let filled = |name: &str| param(&parsed, name).filter(|v| !v.trim().is_empty());
  Some(KickoffLink {
    repo: github_repo(&parsed)?,
    branch: filled("branch")?,
    prompt: filled("prompt")?,
  })
}

/// `url` when it is a `treq://{host}{path}` link.
fn treq_link(url: &str, host: &str, path: &str) -> Option<url::Url> {
  let parsed = url::Url::parse(url).ok()?;
  (parsed.scheme() == "treq" && parsed.host_str() == Some(host) && parsed.path() == path)
    .then_some(parsed)
}

fn param(parsed: &url::Url, name: &str) -> Option<String> {
  parsed
    .query_pairs()
    .find(|(key, _)| key == name)
    .map(|(_, value)| value.into_owned())
}

/// A link's `repo` parameter, when it is a GitHub `owner/repo`.
fn github_repo(parsed: &url::Url) -> Option<String> {
  let repo = param(parsed, "repo")?;
  let (owner, name) = repo.split_once('/')?;
  (is_github_name(owner) && is_github_name(name)).then_some(repo)
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

  #[test]
  fn reads_the_repo_of_a_repo_open_link() {
    assert_eq!(
      parse_repo_open_link("treq://repo/open?repo=tznc%2Ftreq").as_deref(),
      Some("tznc/treq")
    );
    for url in [
      "treq://repo/open",
      "treq://repo/open?repo=tznc",
      "treq://repo/open?repo=..%2Ftreq",
      "treq://repo/close?repo=tznc%2Ftreq",
      "treq://pr/open?repo=tznc%2Ftreq&number=42",
    ] {
      assert_eq!(parse_repo_open_link(url), None, "{url}");
    }
  }

  #[test]
  fn reads_an_agent_kickoff_link() {
    assert_eq!(
      parse_agent_kickoff_link(
        "treq://agent/kickoff?repo=tznc%2Ftreq&branch=claude%2Fx&prompt=src%2Fa.rs%3A4%0A%0Awhy%3F"
      ),
      Some(KickoffLink {
        repo: "tznc/treq".into(),
        branch: "claude/x".into(),
        prompt: "src/a.rs:4\n\nwhy?".into(),
      })
    );
    for url in [
      "treq://agent/kickoff?repo=tznc%2Ftreq&branch=main",
      "treq://agent/kickoff?repo=tznc%2Ftreq&prompt=why",
      "treq://agent/kickoff?repo=tznc%2Ftreq&branch=main&prompt=%20",
      "treq://agent/kickoff?repo=tznc&branch=main&prompt=why",
      "treq://agent/start?repo=tznc%2Ftreq&branch=main&prompt=why",
    ] {
      assert_eq!(parse_agent_kickoff_link(url), None, "{url}");
    }
  }
}
