use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const HEARTBEAT_INTERVAL_MS: u64 = 2_000;
pub const HEARTBEAT_TIMEOUT_MS: u64 = 15_000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentDispatchRequest {
  pub request_id: String,
  pub repo: String,
  pub branch: String,
  pub prompt: String,
  pub mode: String,
  pub agent: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentDispatchResponse {
  pub status: String,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub reason: Option<String>,
}

impl AgentDispatchResponse {
  pub fn handled() -> Self {
    Self {
      status: "handled".to_string(),
      reason: None,
    }
  }

  pub fn not_handled(reason: impl Into<String>) -> Self {
    Self {
      status: "not_handled".to_string(),
      reason: Some(reason.into()),
    }
  }

  /// The app received the request and chose not to act on it.
  pub fn skipped(reason: impl Into<String>) -> Self {
    Self {
      status: "skipped".to_string(),
      reason: Some(reason.into()),
    }
  }

  pub fn error(reason: impl Into<String>) -> Self {
    Self {
      status: "error".to_string(),
      reason: Some(reason.into()),
    }
  }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RegisteredInstance {
  pub instance_id: String,
  pub pid: u32,
  pub started_at: u64,
  pub last_heartbeat_at: u64,
  pub endpoint: String,
  pub windows: Vec<InstanceWindowSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InstanceWindowSnapshot {
  pub window_label: String,
  pub normalized_repo_path: String,
  pub focused: bool,
  pub last_focused_at: Option<u64>,
  /// The window shows this repository as a supporting repository of another
  /// main repository. Windows where it is the main repository win.
  #[serde(default)]
  pub supporting: bool,
}

pub fn now_millis() -> u64 {
  SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .unwrap_or_default()
    .as_millis() as u64
}

pub fn normalize_repo_path(path: &str) -> String {
  std::fs::canonicalize(Path::new(path))
    .ok()
    .and_then(|p| p.to_str().map(|s| s.to_string()))
    .unwrap_or_else(|| path.to_string())
}

#[cfg(test)]
fn prune_stale_instances(instances: &mut Vec<RegisteredInstance>, now: u64, timeout_ms: u64) {
  instances.retain(|instance| now.saturating_sub(instance.last_heartbeat_at) <= timeout_ms);
}

pub fn resolve_target_instance<'a>(
  instances: &'a [RegisteredInstance],
  repo_path: &str,
) -> Option<&'a RegisteredInstance> {
  let normalized_repo = normalize_repo_path(repo_path);
  let has_main_window = instances.iter().any(|instance| {
    instance
      .windows
      .iter()
      .any(|w| w.normalized_repo_path == normalized_repo && !w.supporting)
  });

  let mut focused_match: Option<(&RegisteredInstance, u64)> = None;
  let mut recency_match: Option<(&RegisteredInstance, u64)> = None;
  let mut any_match: Option<(&RegisteredInstance, u64)> = None;

  for instance in instances {
    for window in &instance.windows {
      if window.normalized_repo_path != normalized_repo || (has_main_window && window.supporting) {
        continue;
      }

      if window.focused {
        let score = window.last_focused_at.unwrap_or(0);
        if focused_match.map(|(_, s)| score > s).unwrap_or(true) {
          focused_match = Some((instance, score));
        }
      }

      let score = window.last_focused_at.unwrap_or(0);
      if recency_match.map(|(_, s)| score > s).unwrap_or(true) {
        recency_match = Some((instance, score));
      }

      if any_match
        .map(|(_, s)| instance.last_heartbeat_at > s)
        .unwrap_or(true)
      {
        any_match = Some((instance, instance.last_heartbeat_at));
      }
    }
  }

  focused_match
    .map(|(instance, _)| instance)
    .or_else(|| recency_match.map(|(instance, _)| instance))
    .or_else(|| any_match.map(|(instance, _)| instance))
}

pub fn send_dispatch_request(
  endpoint: &str,
  request: &AgentDispatchRequest,
  connect_timeout: Duration,
  io_timeout: Duration,
) -> Result<AgentDispatchResponse, String> {
  let addr = endpoint
    .parse()
    .map_err(|e| format!("invalid endpoint '{}': {}", endpoint, e))?;
  let mut stream = TcpStream::connect_timeout(&addr, connect_timeout)
    .map_err(|e| format!("failed to connect to endpoint {}: {}", endpoint, e))?;

  stream
    .set_read_timeout(Some(io_timeout))
    .map_err(|e| format!("failed to set read timeout: {}", e))?;
  stream
    .set_write_timeout(Some(io_timeout))
    .map_err(|e| format!("failed to set write timeout: {}", e))?;

  let payload = serde_json::to_vec(request)
    .map_err(|e| format!("failed to serialize dispatch request: {}", e))?;
  stream
    .write_all(&payload)
    .map_err(|e| format!("failed to send dispatch request: {}", e))?;
  stream
    .shutdown(Shutdown::Write)
    .map_err(|e| format!("failed to finish dispatch write: {}", e))?;

  let mut response = String::new();
  stream
    .read_to_string(&mut response)
    .map_err(|e| format!("failed reading dispatch response: {}", e))?;

  serde_json::from_str::<AgentDispatchResponse>(response.trim())
    .map_err(|e| format!("invalid dispatch response payload: {}", e))
}

pub fn bind_ephemeral_listener() -> Result<(TcpListener, String), String> {
  let listener = TcpListener::bind("127.0.0.1:0")
    .map_err(|e| format!("failed to bind local ipc listener: {}", e))?;
  let endpoint = listener
    .local_addr()
    .map_err(|e| format!("failed to read local ipc endpoint: {}", e))?
    .to_string();
  Ok((listener, endpoint))
}

#[cfg(test)]
mod tests {
  use super::*;

  fn make_instance(
    id: &str,
    heartbeat: u64,
    windows: Vec<InstanceWindowSnapshot>,
  ) -> RegisteredInstance {
    RegisteredInstance {
      instance_id: id.to_string(),
      pid: 1,
      started_at: 1,
      last_heartbeat_at: heartbeat,
      endpoint: "127.0.0.1:1".to_string(),
      windows,
    }
  }

  #[test]
  fn resolves_focused_before_recency() {
    let repo = normalize_repo_path("/tmp/repo");
    let instances = vec![
      make_instance(
        "a",
        10,
        vec![InstanceWindowSnapshot {
          window_label: "w1".to_string(),
          normalized_repo_path: repo.clone(),
          focused: false,
          last_focused_at: Some(100),
          supporting: false,
        }],
      ),
      make_instance(
        "b",
        10,
        vec![InstanceWindowSnapshot {
          window_label: "w2".to_string(),
          normalized_repo_path: repo,
          focused: true,
          last_focused_at: Some(50),
          supporting: false,
        }],
      ),
    ];

    let selected = resolve_target_instance(&instances, "/tmp/repo").unwrap();
    assert_eq!(selected.instance_id, "b");
  }

  #[test]
  fn resolves_recency_when_no_focused() {
    let repo = normalize_repo_path("/tmp/repo");
    let instances = vec![
      make_instance(
        "a",
        10,
        vec![InstanceWindowSnapshot {
          window_label: "w1".to_string(),
          normalized_repo_path: repo.clone(),
          focused: false,
          last_focused_at: Some(100),
          supporting: false,
        }],
      ),
      make_instance(
        "b",
        10,
        vec![InstanceWindowSnapshot {
          window_label: "w2".to_string(),
          normalized_repo_path: repo,
          focused: false,
          last_focused_at: Some(50),
          supporting: false,
        }],
      ),
    ];

    let selected = resolve_target_instance(&instances, "/tmp/repo").unwrap();
    assert_eq!(selected.instance_id, "a");
  }

  #[test]
  fn prefers_main_window_over_focused_supporting_window() {
    let repo = normalize_repo_path("/tmp/repo");
    let window = |label: &str, focused: bool, supporting: bool| InstanceWindowSnapshot {
      window_label: label.to_string(),
      normalized_repo_path: repo.clone(),
      focused,
      last_focused_at: Some(if focused { 100 } else { 1 }),
      supporting,
    };
    let instances = vec![
      make_instance("supporting", 10, vec![window("w1", true, true)]),
      make_instance("main", 10, vec![window("w2", false, false)]),
    ];

    let selected = resolve_target_instance(&instances, "/tmp/repo").unwrap();
    assert_eq!(selected.instance_id, "main");
  }

  #[test]
  fn falls_back_to_supporting_window_when_no_main_window() {
    let repo = normalize_repo_path("/tmp/repo");
    let instances = vec![make_instance(
      "supporting",
      10,
      vec![InstanceWindowSnapshot {
        window_label: "w1".to_string(),
        normalized_repo_path: repo,
        focused: false,
        last_focused_at: None,
        supporting: true,
      }],
    )];

    let selected = resolve_target_instance(&instances, "/tmp/repo").unwrap();
    assert_eq!(selected.instance_id, "supporting");
  }

  #[test]
  fn prunes_stale_instances_by_heartbeat_timeout() {
    let mut instances = vec![
      make_instance("alive", 95, vec![]),
      make_instance("stale", 10, vec![]),
    ];

    prune_stale_instances(&mut instances, 100, 10);
    assert_eq!(instances.len(), 1);
    assert_eq!(instances[0].instance_id, "alive");
  }

  #[test]
  fn normalize_repo_path_matches_relative_and_symlink_forms() {
    let temp = tempfile::TempDir::new().expect("temp");
    let target = temp.path().join("repo");
    std::fs::create_dir_all(&target).expect("mkdir");
    let link = temp.path().join("repo-link");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&target, &link).expect("symlink");
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(&target, &link).expect("symlink");

    let canonical = normalize_repo_path(target.to_str().unwrap());
    let linked = normalize_repo_path(link.to_str().unwrap());
    assert_eq!(canonical, linked);
  }
}
