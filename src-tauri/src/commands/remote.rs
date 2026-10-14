use crate::core::feature_preview::PreviewFeature;
use crate::core::remote::{self, SshHost};
use crate::core::remote_local_keys::{self, LocalSshIdentity};
use crate::core::remote_ssh_config::{self, ResolvedSshAlias};
use crate::AppState;
use tauri::State;

fn require_remote_ssh(state: &State<AppState>) -> Result<(), String> {
  crate::commands::feature_preview::require(state, PreviewFeature::RemoteSsh)
}

#[tauri::command]
pub fn list_ssh_hosts(state: State<AppState>) -> Result<Vec<SshHost>, String> {
  require_remote_ssh(&state)?;
  remote::list_configured_hosts()
}

/// Resolves an explicitly-selected `~/.ssh/config` alias into its hostname,
/// port, username, and identity fields (PRD "Treat discovery as autocomplete
/// only; never connect merely because an alias was found"). This performs no
/// network I/O and never grants trust by itself — the UI uses it only to
/// prefill the registration form; the user still supplies and confirms the
/// expected host-key fingerprint before the endpoint is registered.
#[tauri::command]
pub fn resolve_ssh_config_alias(
  state: State<AppState>,
  alias: String,
) -> Result<ResolvedSshAlias, String> {
  require_remote_ssh(&state)?;
  remote_ssh_config::resolve_alias(&alias).map_err(|error| error.to_string())
}

/// Lists the user's existing local SSH public-key identities, for the
/// managed-VM setup flow's identity picker. Never reads or returns private
/// key material - see `core::remote_local_keys`.
#[tauri::command]
pub fn list_local_ssh_identities() -> Result<Vec<LocalSshIdentity>, String> {
  remote_local_keys::list_local_ssh_identities()
}

/// Reads the raw OpenSSH public-key text for a `list_local_ssh_identities`
/// reference, so the managed-VM setup flow can register it with the
/// control plane. Never reads or returns private key material - see
/// `core::remote_local_keys::read_local_public_key`.
#[tauri::command]
pub fn read_local_ssh_public_key(reference: String) -> Result<String, String> {
  remote_local_keys::read_local_public_key(&reference)
}

/// Reads the host-key fingerprint a user-managed host presents, so the setup
/// flow can ask the user to confirm it before pinning it. Never
/// authenticates - see `core::remote_ssh_host_key_probe`.
#[tauri::command]
pub async fn probe_ssh_host_key(hostname: String, port: u16) -> Result<String, String> {
  crate::core::remote_ssh_host_key_probe::probe_host_key_fingerprint(&hostname, port).await
}
