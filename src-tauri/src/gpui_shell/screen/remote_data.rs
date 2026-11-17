//! What the remote dialogs (`RemoteSetupDialog`, `RemoteTerminalDialog`)
//! read from the backend: the SSH setup of "Your own VM" and the persistent
//! sessions on a connected endpoint.

use tauri::Manager;

use super::{account_data::AccountHost, checklist::ToolsHost};
use crate::{
  core::{
    feature_preview::{self, PreviewFeature},
    pty_remote_supervisor::PtySessionInfo,
    remote,
    remote_control_plane::SshEndpoint,
    remote_local_keys::{self, LocalSshIdentity},
    remote_ssh_config::{self, ResolvedSshAlias},
    remote_ssh_host_key_probe,
    remote_ssh_transport::{CancellationToken, ExecLimits},
  },
  gpui_shell::native::Backend,
};

/// What the remote dialogs ask of the backend; faked in tests. Everything
/// but `open_url` blocks, so it is called off the main thread.
pub trait RemoteHost: ToolsHost + AccountHost {
  /// The `~/.ssh/config` aliases the hostname field suggests; none while
  /// the Remote SSH preview is off.
  fn ssh_aliases(&self) -> Vec<String>;
  /// The public keys in `~/.ssh`, for the SSH key select.
  fn ssh_identities(&self) -> Vec<LocalSshIdentity>;
  /// The host key fingerprint `hostname:port` presents, unauthenticated.
  fn probe_host_key(&self, hostname: &str, port: u16) -> Result<String, String>;
  /// `Autofill from alias`: one alias's connection fields.
  fn resolve_alias(&self, alias: &str) -> Result<ResolvedSshAlias, String>;
  /// The `pty-remote` sessions running on the endpoint for a workspace.
  fn persistent_sessions(
    &self,
    endpoint: &SshEndpoint,
    repo: &str,
    workspace: &str,
  ) -> Result<Vec<PtySessionInfo>, String>;
}

impl RemoteHost for Backend {
  fn ssh_aliases(&self) -> Vec<String> {
    if !self.with_db(|db| feature_preview::is_enabled(db, PreviewFeature::RemoteSsh)) {
      return Vec::new();
    }
    remote::list_configured_hosts()
      .map(|hosts| hosts.into_iter().map(|host| host.alias).collect())
      .unwrap_or_default()
  }

  fn ssh_identities(&self) -> Vec<LocalSshIdentity> {
    remote_local_keys::list_local_ssh_identities().unwrap_or_default()
  }

  fn probe_host_key(&self, hostname: &str, port: u16) -> Result<String, String> {
    crate::gpui_shell::block_on(remote_ssh_host_key_probe::probe_host_key_fingerprint(
      hostname, port,
    ))
  }

  fn resolve_alias(&self, alias: &str) -> Result<ResolvedSshAlias, String> {
    self.with_db(|db| feature_preview::require(db, PreviewFeature::RemoteSsh))?;
    remote_ssh_config::resolve_alias(alias).map_err(|error| error.to_string())
  }

  fn persistent_sessions(
    &self,
    endpoint: &SshEndpoint,
    repo: &str,
    workspace: &str,
  ) -> Result<Vec<PtySessionInfo>, String> {
    self.with_db(|db| feature_preview::require_remote_for_source(db, &endpoint.source))?;
    let exec = self
      .app()
      .state::<crate::commands::remote_control::RemoteExecState>();
    let cancellation = CancellationToken::new();
    crate::gpui_shell::block_on(remote::execute_remote_command::<Vec<PtySessionInfo>>(
      &exec.0,
      endpoint,
      remote::TreqCommandRequest::PtyList {
        backend: endpoint.terminal_backend,
        repo: repo.to_string(),
        workspace: Some(workspace.to_string()),
      },
      ExecLimits::default(),
      &cancellation,
    ))
    .map_err(|error| error.to_string())
  }
}

/// `pickDefaultKeyReference`: `id_ed25519` if present, else the first key.
pub fn default_key(identities: &[LocalSshIdentity]) -> Option<String> {
  identities
    .iter()
    .find(|identity| identity.label == "id_ed25519")
    .or(identities.first())
    .map(|identity| identity.reference.clone())
}

#[cfg(test)]
mod tests {
  use super::*;

  fn identity(label: &str) -> LocalSshIdentity {
    LocalSshIdentity {
      reference: format!("/home/me/.ssh/{label}.pub"),
      label: label.into(),
      fingerprint_sha256: String::new(),
      algorithm: "ssh-ed25519".into(),
    }
  }

  #[test]
  fn prefers_the_ed25519_key() {
    let keys = [identity("id_rsa"), identity("id_ed25519")];
    assert_eq!(
      default_key(&keys).as_deref(),
      Some("/home/me/.ssh/id_ed25519.pub")
    );
  }

  #[test]
  fn falls_back_to_the_first_key() {
    let keys = [identity("work"), identity("id_rsa")];
    assert_eq!(
      default_key(&keys).as_deref(),
      Some("/home/me/.ssh/work.pub")
    );
  }

  #[test]
  fn has_no_default_without_keys() {
    assert_eq!(default_key(&[]), None);
  }
}
