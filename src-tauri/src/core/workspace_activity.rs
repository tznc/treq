//! What runs on the cloud workspace, from the idle controller's last probe
//! (`get_workspace_activity`), and the process actions the cloud card offers
//! (`pin_process`, `unpin_process`, `kill_process` on remote-instance).
//!
//! The control plane stops a workspace after a fixed idle time. Only ssh
//! sessions, agent turns, terminals and processes the user pinned keep it
//! awake; a pin keeps the machine running, so it is billed.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::supabase::{ApiError, Supabase};

/// A process the last probe saw.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BackgroundProcess {
  pub pid: u32,
  pub command: String,
  /// `service`, `agent`, `terminal` or `orphan`.
  pub origin: String,
  #[serde(default)]
  pub cpu: f64,
  #[serde(default)]
  pub ports: Vec<u16>,
  #[serde(default)]
  pub pinned: bool,
}

/// An unexpired pin.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProcessPin {
  pub id: String,
  #[serde(default)]
  pub pid: Option<u32>,
  #[serde(default)]
  pub command: Option<String>,
  #[serde(default)]
  pub pattern: Option<String>,
  pub expires_at: String,
}

/// `get_workspace_activity()`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WorkspaceActivity {
  /// When the workspace stops if it stays idle; none while it is stopped.
  pub stops_at: Option<String>,
  pub idle_timeout_min: Option<u32>,
  pub processes: Vec<BackgroundProcess>,
  pub pins: Vec<ProcessPin>,
}

impl WorkspaceActivity {
  /// The pin that covers `process`, by pid or by a pattern in its command.
  pub fn pin_for(&self, process: &BackgroundProcess) -> Option<&ProcessPin> {
    self.pins.iter().find(|pin| match (&pin.pid, &pin.pattern) {
      (Some(pid), _) => *pid == process.pid,
      (None, Some(pattern)) => process.command.contains(pattern.as_str()),
      _ => false,
    })
  }
}

/// Parses `get_workspace_activity()`; `null` (no workspace) is `None`.
pub fn parse_activity(value: Value) -> Result<Option<WorkspaceActivity>, String> {
  if value.is_null() {
    return Ok(None);
  }
  serde_json::from_value(value)
    .map(Some)
    .map_err(|e| format!("invalid workspace activity: {e}"))
}

pub async fn fetch_activity(supabase: &Supabase) -> Result<Option<WorkspaceActivity>, ApiError> {
  let value: Value = supabase.rpc("get_workspace_activity", &json!({})).await?;
  parse_activity(value).map_err(|e| ApiError::new("invalid_activity", e))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn parses_the_rpc_and_finds_pins() {
    let activity = parse_activity(json!({
      "instance_id": "i1", "status": "ready", "stops_at": "2026-10-28T12:15:00Z",
      "idle_timeout_min": 15, "active": false, "reasons": [],
      "processes": [
        {"pid": 500, "command": "node server.js", "origin": "orphan", "cpu": 12.5, "ports": [3000], "pinned": true},
        {"pid": 501, "command": "python worker.py", "origin": "agent", "cpu": 0, "ports": [], "pinned": false}
      ],
      "pins": [{"id": "p1", "pid": null, "command": null, "pattern": "server.js", "expires_at": "2026-10-28T20:00:00Z"}]
    }))
    .unwrap()
    .expect("activity");
    assert_eq!(activity.idle_timeout_min, Some(15));
    assert_eq!(activity.processes[0].ports, [3000]);
    assert_eq!(
      activity
        .pin_for(&activity.processes[0])
        .map(|p| p.id.as_str()),
      Some("p1")
    );
    assert_eq!(activity.pin_for(&activity.processes[1]), None);
    assert_eq!(parse_activity(Value::Null).unwrap(), None);
  }
}
