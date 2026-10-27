//! The coordinator's real db and network: the app db's `agent_messages`
//! ledger, and the other treq instances found in each repository's
//! instance registry, reached over their IPC endpoints.

use super::coordinator::{Ledger, Network};
use super::hub::{Envelope, Report, SessionInfo};
use super::ipc;
use super::model::State;
use super::protocol::{Op, Request, KIND};
use crate::agent_dispatch;
use crate::local_db::{self, AgentMessageTarget, NewAgentMessage};
use std::time::Duration;

/// How long a peer may take to answer.
const PEER_TIMEOUT: Duration = Duration::from_secs(2);

pub struct DbLedger;

impl Ledger for DbLedger {
  fn insert(&self, repo: &str, message: &Envelope, targets: &[(String, String)]) -> Result<(), String> {
    let targets: Vec<AgentMessageTarget> = targets
      .iter()
      .map(|(handle, via)| AgentMessageTarget {
        handle: handle.clone(),
        via: via.clone(),
      })
      .collect();
    local_db::insert_agent_message(
      repo,
      &NewAgentMessage {
        id: message.id.clone(),
        kind: message.kind,
        from: message.from.clone(),
        body: message.body.clone(),
        reply_to: message.reply_to.clone(),
        sent_at_ms: message.sent_at_ms,
        timeout: Duration::from_millis(message.timeout_ms),
      },
      &targets,
    )
  }

  fn update(
    &self,
    repo: &str,
    id: &str,
    target: &str,
    state: State,
    mode: Option<&str>,
    now_ms: u64,
  ) -> Result<(), String> {
    local_db::update_agent_message_state(repo, id, target, state, mode, now_ms).map(drop)
  }
}

/// This instance, as `endpoint` in the registries.
pub struct InstanceNetwork {
  pub endpoint: String,
}

impl InstanceNetwork {
  fn call(&self, endpoint: &str, op: Op) -> Result<serde_json::Value, String> {
    let request = Request {
      kind: KIND.into(),
      request_id: format!("peer-{}", agent_dispatch::now_millis()),
      repo: String::new(),
      op,
    };
    ipc::call(endpoint, &request, PEER_TIMEOUT)
  }
}

impl Network for InstanceNetwork {
  fn own_endpoint(&self) -> String {
    self.endpoint.clone()
  }

  fn peers(&self, repos: &[String]) -> Vec<String> {
    let now = agent_dispatch::now_millis();
    let mut endpoints: Vec<String> = repos
      .iter()
      .filter(|repo| std::path::Path::new(repo).is_dir())
      .filter_map(|repo| local_db::list_instance_registry(repo).ok())
      .flatten()
      .filter(|instance| {
        now.saturating_sub(instance.last_heartbeat_at) <= agent_dispatch::HEARTBEAT_TIMEOUT_MS
      })
      .map(|instance| instance.endpoint)
      .filter(|endpoint| *endpoint != self.endpoint)
      .collect();
    endpoints.sort();
    endpoints.dedup();
    endpoints
  }

  fn list(&self, endpoint: &str) -> Result<Vec<SessionInfo>, String> {
    let data = self.call(
      endpoint,
      Op::List {
        repo: None,
        local_only: true,
      },
    )?;
    let agents = data["agents"].as_array().cloned().unwrap_or_default();
    Ok(
      agents
        .into_iter()
        .filter_map(|agent| {
          Some(SessionInfo {
            pty_id: agent["pty_id"].as_str()?.to_string(),
            handle: agent["handle"].as_str()?.to_string(),
            harness: agent["harness"].as_str()?.to_string(),
            repo: agent["repo"].as_str()?.to_string(),
            workspace: agent["workspace"].as_str().map(str::to_string),
            ledger: agent["ledger"].as_str().map(str::to_string),
          })
        })
        .collect(),
    )
  }

  fn deliver(&self, endpoint: &str, envelope: &Envelope) -> Result<(), String> {
    self
      .call(
        endpoint,
        Op::Deliver {
          envelope: envelope.clone(),
        },
      )
      .map(drop)
  }

  fn mirror(&self, envelope: &Envelope) -> Result<(), String> {
    super::service::mirror(envelope)
  }

  fn report(&self, endpoint: &str, report: &Report) -> Result<(), String> {
    self
      .call(
        endpoint,
        Op::Report {
          report: report.clone(),
        },
      )
      .map(drop)
  }
}

#[cfg(test)]
mod tests {
  use super::super::model::Kind;
  use super::*;
  use crate::agent_dispatch::RegisteredInstance;
  use tempfile::TempDir;

  fn instance(id: &str, endpoint: &str, heartbeat: u64) -> RegisteredInstance {
    RegisteredInstance {
      instance_id: id.into(),
      pid: 1,
      started_at: 1,
      last_heartbeat_at: heartbeat,
      endpoint: endpoint.into(),
      windows: Vec::new(),
    }
  }

  #[test]
  fn peers_are_the_live_instances_of_each_repo_but_this_one() {
    let (a, b) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    let (a, b) = (a.path().to_str().unwrap(), b.path().to_str().unwrap());
    let now = agent_dispatch::now_millis();
    local_db::upsert_instance_registry(a, instance("me", "127.0.0.1:1", now)).unwrap();
    local_db::upsert_instance_registry(a, instance("x", "127.0.0.1:2", now)).unwrap();
    local_db::upsert_instance_registry(b, instance("x", "127.0.0.1:2", now)).unwrap();
    local_db::upsert_instance_registry(b, instance("y", "127.0.0.1:3", now)).unwrap();
    local_db::upsert_instance_registry(b, instance("dead", "127.0.0.1:4", 1)).unwrap();

    let network = InstanceNetwork {
      endpoint: "127.0.0.1:1".into(),
    };
    assert_eq!(
      network.peers(&[a.to_string(), b.to_string()]),
      vec!["127.0.0.1:2".to_string(), "127.0.0.1:3".to_string()]
    );
  }

  #[test]
  fn a_repo_with_no_db_has_no_peers() {
    let network = InstanceNetwork {
      endpoint: "127.0.0.1:1".into(),
    };
    assert!(network.peers(&["/no/such/repo".to_string()]).is_empty());
  }

  #[test]
  fn the_ledger_records_into_the_repos_app_db() {
    let dir = TempDir::new().unwrap();
    let repo = dir.path().to_str().unwrap();
    let message = Envelope {
      id: "m-1".into(),
      kind: Kind::Ask,
      from: "codex-1".into(),
      to: String::new(),
      to_pty: String::new(),
      body: "rebase done?".into(),
      reply_to: None,
      sent_at_ms: 10,
      timeout_ms: 30_000,
      origin: "127.0.0.1:1".into(),
      origin_repo: repo.into(),
    };
    DbLedger
      .insert(repo, &message, &[("claude-2".into(), "local".into())])
      .unwrap();
    DbLedger
      .update(repo, "m-1", "claude-2", State::Delivered, Some("wake"), 20)
      .unwrap();

    let rows = local_db::agent_message_rows(repo, "m-1").unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].state, State::Delivered);
    assert_eq!(rows[0].mode.as_deref(), Some("wake"));
    assert_eq!(rows[0].timeout_ms, 30_000);
  }

  #[test]
  fn listing_an_unreachable_peer_is_an_error() {
    let network = InstanceNetwork {
      endpoint: "127.0.0.1:1".into(),
    };
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = listener.local_addr().unwrap().to_string();
    drop(listener);
    assert!(network.list(&endpoint).is_err());
  }
}
