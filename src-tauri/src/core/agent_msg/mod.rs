//! `treq agent msg`: messages between the agents treq launched, in this
//! window, another treq instance on this machine, or a remote host.
//!
//! The instance whose agent sends a message coordinates it: it assigns the
//! id, resolves the targets, delivers to each one and records every state
//! change in its app db (`local_db::agent_messages`) and the system log
//! (target [`LOG_TARGET`]). A target's instance only queues the message and
//! wakes its agent.

pub mod coordinator;
pub mod delivery;
pub mod hub;
pub mod idle;
pub mod model;
pub mod route;

/// The `log` target every state change is written under, so the system log
/// keeps the delivery history after the db row is deleted.
pub const LOG_TARGET: &str = "treq::agent_msg";

/// One system log line: `event=<event> id=<id>` then `key=value` pairs.
/// Values with spaces are quoted so the line stays greppable.
pub fn log_line(event: &str, id: &str, fields: &[(&str, &str)]) -> String {
  let mut line = format!("event={event} id={id}");
  for (key, value) in fields {
    line.push(' ');
    line.push_str(key);
    line.push('=');
    if value.is_empty() || value.contains([' ', '"', '=']) {
      line.push('"');
      line.push_str(&value.replace('\\', "\\\\").replace('"', "\\\""));
      line.push('"');
    } else {
      line.push_str(value);
    }
  }
  line
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn log_lines_are_key_value_pairs() {
    assert_eq!(
      log_line("delivered", "m-1", &[("to", "claude-2"), ("mode", "wake")]),
      "event=delivered id=m-1 to=claude-2 mode=wake"
    );
  }

  #[test]
  fn log_values_with_spaces_or_quotes_are_quoted() {
    assert_eq!(
      log_line("queued", "m-1", &[("via", "ssh:build box"), ("note", "say \"hi\"")]),
      "event=queued id=m-1 via=\"ssh:build box\" note=\"say \\\"hi\\\"\""
    );
  }
}
