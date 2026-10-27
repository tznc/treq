//! Which agent terminals a message goes to, across every treq instance the
//! sender's instance can see.

use super::hub::SessionInfo;
use serde::{Deserialize, Serialize};

/// How the sending instance reaches a terminal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "endpoint", rename_all = "snake_case")]
pub enum Via {
  /// A terminal of the sending instance itself.
  Local,
  /// A terminal of another treq instance on this machine, by its IPC endpoint.
  Socket(String),
}

impl Via {
  /// As the ledger's `via` column stores it.
  pub fn label(&self) -> String {
    match self {
      Via::Local => "local".into(),
      Via::Socket(endpoint) => format!("socket:{endpoint}"),
    }
  }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Peer {
  pub via: Via,
  pub session: SessionInfo,
}

/// Whether `repo` (a path, directory name or trailing path such as
/// `org/app`) names `path`.
pub fn repo_matches(path: &str, repo: &str) -> bool {
  let path = path.trim_end_matches('/');
  let repo = repo.trim_end_matches('/');
  if repo.is_empty() {
    return false;
  }
  path == repo || path.ends_with(&format!("/{repo}"))
}

/// The targets of a message from `from_pty`: the agent called `to`, or with
/// `all` every agent but the sender. `repo` narrows either to one repository.
pub fn resolve(
  peers: &[Peer],
  from_pty: &str,
  to: Option<&str>,
  all: bool,
  repo: Option<&str>,
) -> Result<Vec<Peer>, String> {
  let in_repo = |peer: &&Peer| repo.is_none_or(|repo| repo_matches(&peer.session.repo, repo));
  match (to, all) {
    (Some(_), true) => Err("give an agent or --all, not both".into()),
    (None, false) => Err("give the agent to message, or --all".into()),
    (None, true) => {
      let found: Vec<Peer> = peers
        .iter()
        .filter(|peer| peer.session.pty_id != from_pty)
        .filter(in_repo)
        .cloned()
        .collect();
      if found.is_empty() {
        return Err("no other agents to message. See treq agent msg --list".into());
      }
      Ok(found)
    }
    (Some(handle), false) => {
      let found: Vec<&Peer> = peers
        .iter()
        .filter(|peer| peer.session.handle == handle)
        .filter(in_repo)
        .collect();
      match found.as_slice() {
        [] => Err(format!(
          "no agent called {handle}. See treq agent msg --list"
        )),
        [peer] if peer.session.pty_id == from_pty => {
          Err("an agent cannot message itself".into())
        }
        [peer] => Ok(vec![(*peer).clone()]),
        many => {
          let repos: Vec<&str> = many.iter().map(|peer| peer.session.repo.as_str()).collect();
          Err(format!(
            "{handle} runs in more than one repository ({}). Pick one with --repo",
            repos.join(", ")
          ))
        }
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn peer(handle: &str, repo: &str, via: Via) -> Peer {
    Peer {
      via,
      session: SessionInfo {
        pty_id: format!("pty-{handle}-{repo}"),
        handle: handle.into(),
        harness: handle.split('-').next().unwrap().into(),
        repo: repo.into(),
        workspace: None,
      },
    }
  }

  fn peers() -> Vec<Peer> {
    vec![
      peer("codex-1", "/src/app", Via::Local),
      peer("claude-2", "/src/app", Via::Local),
      peer("copilot-3", "/src/app", Via::Socket("127.0.0.1:7".into())),
      peer("claude-2", "/src/api", Via::Socket("127.0.0.1:8".into())),
    ]
  }

  fn handles(found: &[Peer]) -> Vec<String> {
    found
      .iter()
      .map(|p| format!("{}@{}", p.session.handle, p.session.repo))
      .collect()
  }

  const SENDER: &str = "pty-codex-1-/src/app";

  #[test]
  fn names_a_repo_by_path_directory_name_or_trailing_path() {
    assert!(repo_matches("/src/org/app", "/src/org/app"));
    assert!(repo_matches("/src/org/app", "app"));
    assert!(repo_matches("/src/org/app", "org/app"));
    assert!(repo_matches("/src/org/app/", "app"));
    assert!(!repo_matches("/src/org/app", "pp"));
    assert!(!repo_matches("/src/org/myapp", "app"));
  }

  #[test]
  fn sends_to_the_one_agent_with_that_handle() {
    let found = resolve(&peers(), SENDER, Some("copilot-3"), false, None).unwrap();
    assert_eq!(handles(&found), vec!["copilot-3@/src/app"]);
    assert_eq!(found[0].via, Via::Socket("127.0.0.1:7".into()));
  }

  #[test]
  fn a_handle_in_two_repos_needs_the_repo() {
    let error = resolve(&peers(), SENDER, Some("claude-2"), false, None).unwrap_err();
    assert!(error.contains("--repo"), "{error}");
    assert!(error.contains("/src/api") && error.contains("/src/app"), "{error}");

    let found = resolve(&peers(), SENDER, Some("claude-2"), false, Some("api")).unwrap();
    assert_eq!(handles(&found), vec!["claude-2@/src/api"]);
  }

  #[test]
  fn an_unknown_handle_is_an_error_that_points_at_list() {
    let error = resolve(&peers(), SENDER, Some("gemini-9"), false, None).unwrap_err();
    assert!(error.contains("gemini-9") && error.contains("--list"), "{error}");
  }

  #[test]
  fn all_sends_to_every_agent_but_the_sender() {
    let found = resolve(&peers(), SENDER, None, true, None).unwrap();
    assert_eq!(
      handles(&found),
      vec!["claude-2@/src/app", "copilot-3@/src/app", "claude-2@/src/api"]
    );
  }

  #[test]
  fn all_with_a_repo_keeps_that_repo() {
    let found = resolve(&peers(), SENDER, None, true, Some("app")).unwrap();
    assert_eq!(handles(&found), vec!["claude-2@/src/app", "copilot-3@/src/app"]);
  }

  #[test]
  fn all_with_nobody_else_is_an_error() {
    let lonely = vec![peer("codex-1", "/src/app", Via::Local)];
    assert!(resolve(&lonely, SENDER, None, true, None).is_err());
  }

  #[test]
  fn needs_exactly_one_of_a_handle_and_all() {
    assert!(resolve(&peers(), SENDER, None, false, None).is_err());
    assert!(resolve(&peers(), SENDER, Some("claude-2"), true, None).is_err());
  }

  #[test]
  fn an_agent_cannot_message_itself() {
    assert!(resolve(&peers(), SENDER, Some("codex-1"), false, None).is_err());
  }

  #[test]
  fn via_labels_match_the_ledger() {
    assert_eq!(Via::Local.label(), "local");
    assert_eq!(Via::Socket("127.0.0.1:7".into()).label(), "socket:127.0.0.1:7");
  }
}
