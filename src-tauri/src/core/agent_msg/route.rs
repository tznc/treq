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

/// Whether project `repos` holds the session repository `repo`.
/// A remote session names its repository by its saved id
/// (`remote_records::repository_id`), the project by the window's key.
pub fn in_project(repos: &[String], repo: &str) -> bool {
  repos.iter().any(|entry| {
    entry == repo
      || crate::core::projects::remote_repo(entry).is_some_and(|remote| remote.saved_id() == repo)
  })
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
/// `project`, the sender's project repositories, keeps `all` inside them and
/// settles a handle that runs both inside and outside it; a direct message
/// may still leave it.
pub fn resolve(
  peers: &[Peer],
  from_pty: &str,
  to: Option<&str>,
  all: bool,
  repo: Option<&str>,
  project: Option<&[String]>,
) -> Result<Vec<Peer>, String> {
  let project = project.filter(|repos| !repos.is_empty());
  let in_scope = |peer: &Peer| project.is_none_or(|repos| in_project(repos, &peer.session.repo));
  let in_repo = |peer: &&Peer| repo.is_none_or(|repo| repo_matches(&peer.session.repo, repo));
  match (to, all) {
    (Some(_), true) => Err("give an agent or --all, not both".into()),
    (None, false) => Err("give the agent to message, or --all".into()),
    (None, true) => {
      let found: Vec<Peer> = peers
        .iter()
        .filter(|peer| peer.session.pty_id != from_pty)
        .filter(in_repo)
        .filter(|peer| in_scope(peer))
        .cloned()
        .collect();
      if found.is_empty() {
        return Err(
          match project {
            Some(_) => {
              "no other agents in this project. See treq agent msg --list, or add --everywhere"
            }
            None => "no other agents to message. See treq agent msg --list",
          }
          .into(),
        );
      }
      Ok(found)
    }
    (Some(handle), false) => {
      let mut found: Vec<&Peer> = peers
        .iter()
        .filter(|peer| peer.session.handle == handle)
        .filter(in_repo)
        .collect();
      if found.len() > 1 && found.iter().filter(|peer| in_scope(peer)).count() == 1 {
        found.retain(|peer| in_scope(peer));
      }
      match found.as_slice() {
        [] => Err(format!(
          "no agent called {handle}. See treq agent msg --list"
        )),
        [peer] if peer.session.pty_id == from_pty => Err("an agent cannot message itself".into()),
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
        ledger: None,
        project: Vec::new(),
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
    let found = resolve(&peers(), SENDER, Some("copilot-3"), false, None, None).unwrap();
    assert_eq!(handles(&found), vec!["copilot-3@/src/app"]);
    assert_eq!(found[0].via, Via::Socket("127.0.0.1:7".into()));
  }

  #[test]
  fn a_handle_in_two_repos_needs_the_repo() {
    let error = resolve(&peers(), SENDER, Some("claude-2"), false, None, None).unwrap_err();
    assert!(error.contains("--repo"), "{error}");
    assert!(
      error.contains("/src/api") && error.contains("/src/app"),
      "{error}"
    );

    let found = resolve(&peers(), SENDER, Some("claude-2"), false, Some("api"), None).unwrap();
    assert_eq!(handles(&found), vec!["claude-2@/src/api"]);
  }

  #[test]
  fn an_unknown_handle_is_an_error_that_points_at_list() {
    let error = resolve(&peers(), SENDER, Some("gemini-9"), false, None, None).unwrap_err();
    assert!(
      error.contains("gemini-9") && error.contains("--list"),
      "{error}"
    );
  }

  #[test]
  fn all_sends_to_every_agent_but_the_sender() {
    let found = resolve(&peers(), SENDER, None, true, None, None).unwrap();
    assert_eq!(
      handles(&found),
      vec![
        "claude-2@/src/app",
        "copilot-3@/src/app",
        "claude-2@/src/api"
      ]
    );
  }

  #[test]
  fn all_with_a_repo_keeps_that_repo() {
    let found = resolve(&peers(), SENDER, None, true, Some("app"), None).unwrap();
    assert_eq!(
      handles(&found),
      vec!["claude-2@/src/app", "copilot-3@/src/app"]
    );
  }

  #[test]
  fn all_with_nobody_else_is_an_error() {
    let lonely = vec![peer("codex-1", "/src/app", Via::Local)];
    assert!(resolve(&lonely, SENDER, None, true, None, None).is_err());
  }

  #[test]
  fn needs_exactly_one_of_a_handle_and_all() {
    assert!(resolve(&peers(), SENDER, None, false, None, None).is_err());
    assert!(resolve(&peers(), SENDER, Some("claude-2"), true, None, None).is_err());
  }

  #[test]
  fn an_agent_cannot_message_itself() {
    assert!(resolve(&peers(), SENDER, Some("codex-1"), false, None, None).is_err());
  }

  const PROJECT: &[&str] = &["/src/app", "ssh:box:gen2:/srv/infra"];

  fn project() -> Vec<String> {
    PROJECT.iter().map(|r| r.to_string()).collect()
  }

  #[test]
  fn a_project_holds_its_local_repos_and_its_remote_ones_by_saved_id() {
    let project = project();
    assert!(in_project(&project, "/src/app"));
    assert!(in_project(&project, "remote-repo:box:gen2:/srv/infra"));
    assert!(!in_project(&project, "/src/api"));
    assert!(!in_project(&project, "remote-repo:box:gen3:/srv/infra"));
  }

  #[test]
  fn all_stays_in_the_senders_project() {
    let mut peers = peers();
    peers.push(peer(
      "codex-4",
      "remote-repo:box:gen2:/srv/infra",
      Via::Local,
    ));
    let project = project();
    let found = resolve(&peers, SENDER, None, true, None, Some(&project)).unwrap();
    assert_eq!(
      handles(&found),
      vec![
        "claude-2@/src/app",
        "copilot-3@/src/app",
        "codex-4@remote-repo:box:gen2:/srv/infra"
      ]
    );
  }

  #[test]
  fn all_with_nobody_in_the_project_points_at_everywhere() {
    let lonely = vec![
      peer("codex-1", "/src/app", Via::Local),
      peer("claude-2", "/src/api", Via::Local),
    ];
    let project = project();
    let error = resolve(&lonely, SENDER, None, true, None, Some(&project)).unwrap_err();
    assert!(error.contains("--everywhere"), "{error}");
  }

  #[test]
  fn a_direct_message_leaves_the_project() {
    let project = project();
    let found = resolve(
      &peers(),
      SENDER,
      Some("claude-2"),
      false,
      Some("api"),
      Some(&project),
    )
    .unwrap();
    assert_eq!(handles(&found), vec!["claude-2@/src/api"]);
  }

  #[test]
  fn a_handle_in_the_project_wins_over_one_outside_it() {
    let project = project();
    let found = resolve(
      &peers(),
      SENDER,
      Some("claude-2"),
      false,
      None,
      Some(&project),
    )
    .unwrap();
    assert_eq!(handles(&found), vec!["claude-2@/src/app"]);
  }

  #[test]
  fn via_labels_match_the_ledger() {
    assert_eq!(Via::Local.label(), "local");
    assert_eq!(
      Via::Socket("127.0.0.1:7".into()).label(),
      "socket:127.0.0.1:7"
    );
  }
}
