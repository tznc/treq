//! The desktop app's Supabase account, owned by Rust ([`core::cloud`]).
//!
//! In a GPUI shell window, Rust restores and refreshes the session, trades
//! the sign-in deep link for one, reads the subscription, runs the cloud
//! workspace flows and renews their certificates. GPUI draws from
//! [`Cloud::snapshot`]; the webview mirrors it from [`ACCOUNT_EVENT`] (and
//! [`cloud_account`] at startup) and asks for changes with these commands.
//!
//! [`core::cloud`]: crate::core::cloud

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use tauri::{AppHandle, Emitter, Manager};

use super::remote_control::{self, RemoteExecState};
use crate::{
  core::{
    auth::{SessionStore, SESSION_SETTING},
    cloud::{AccountSnapshot, Cloud, CloudEffects, ManagedEndpointEvent},
    managed::MANAGED_REPOSITORIES_ROOT,
    remote::{self, MachineUsageReport, TreqCommandRequest},
    remote_control_plane::SshEndpoint,
    remote_local_keys::{self, LocalSshIdentity},
    remote_ssh_transport::{CancellationToken, CutoffReason, ExecLimits},
    supabase::{desktop_env, Supabase},
  },
  lock_ext::LockExt,
  AppState,
};

/// The webview's mirror of [`AccountSnapshot`].
pub const ACCOUNT_EVENT: &str = "supabase-account";
/// A managed endpoint to adopt ([`ManagedEndpointEvent`]).
pub const MANAGED_ENDPOINT_EVENT: &str = "managed-endpoint";

/// Managed while Rust owns the account (GPUI shell windows).
pub struct CloudState {
  pub cloud: Arc<Cloud>,
  effects: Arc<AppEffects>,
}

/// The session's tokens, in the app database.
#[cfg_attr(not(feature = "gpui-shell"), allow(dead_code))]
struct DbStore(AppHandle);

impl SessionStore for DbStore {
  fn load(&self) -> Option<String> {
    let state = self.0.try_state::<AppState>()?;
    let db = state.db.lock_or_recover();
    db.get_setting(SESSION_SETTING)
      .ok()
      .flatten()
      .filter(|v| !v.is_empty())
  }

  fn save(&self, value: &str) {
    if let Some(state) = self.0.try_state::<AppState>() {
      if let Err(error) = state
        .db
        .lock_or_recover()
        .set_setting(SESSION_SETTING, value)
      {
        log::error!("saving the Supabase session: {error}");
      }
    }
  }
}

struct AppEffects {
  app: AppHandle,
  /// The window that started the last cloud workspace action, which
  /// adopts the endpoint it connects to.
  window: Mutex<Option<String>>,
}

#[async_trait]
impl CloudEffects for AppEffects {
  fn changed(&self, snapshot: &AccountSnapshot) {
    let _ = self.app.emit(ACCOUNT_EVENT, snapshot);
    #[cfg(feature = "gpui-shell")]
    #[cfg(desktop)]
    crate::gpui_shell::account_changed(&self.app, snapshot.clone());
  }

  fn access_token(&self, supabase_url: &str, token: Option<&str>) {
    if let Some(state) = self.app.try_state::<RemoteExecState>() {
      state.0.set_relay_access_token(token.map(str::to_string));
    }
    crate::linear::set_proxy_session(
      token.map(|_| supabase_url.to_string()),
      token.map(str::to_string),
    );
  }

  fn managed_endpoint(&self, event: ManagedEndpointEvent) {
    let window = self.window.lock_or_recover().clone();
    let result = match (&event, window) {
      (ManagedEndpointEvent::Connected { .. }, Some(label)) => self.app.emit_to(
        tauri::EventTarget::webview_window(&label),
        MANAGED_ENDPOINT_EVENT,
        &event,
      ),
      _ => self.app.emit(MANAGED_ENDPOINT_EVENT, &event),
    };
    if let Err(error) = result {
      log::warn!("failed to send a managed endpoint: {error}");
    }
  }

  async fn cut_off(&self, endpoint_id: &str, reason: CutoffReason) {
    remote_control::force_cutoff(&self.app, endpoint_id, reason).await;
  }

  async fn cut_off_managed(&self) {
    remote_control::cut_off_managed(&self.app).await;
  }

  async fn clear_cutoff(&self, endpoint_id: &str) {
    if let Some(state) = self.app.try_state::<RemoteExecState>() {
      state.0.clear_cutoff(endpoint_id).await;
    }
  }

  fn local_identities(&self) -> Result<Vec<LocalSshIdentity>, String> {
    remote_local_keys::list_local_ssh_identities()
  }

  fn read_public_key(&self, key_reference: &str) -> Result<String, String> {
    remote_local_keys::read_local_public_key(key_reference)
  }

  async fn machine_usage(&self, endpoint: &SshEndpoint) -> Result<MachineUsageReport, String> {
    let pool = self.app.state::<RemoteExecState>().0.clone();
    remote::execute_remote_command(
      &pool,
      endpoint,
      TreqCommandRequest::MachineUsage {
        root: MANAGED_REPOSITORIES_ROOT.to_string(),
      },
      ExecLimits::default(),
      &CancellationToken::new(),
    )
    .await
    .map_err(|error| error.to_string())
  }
}

/// Takes the account over from the webview: restores the session and keeps
/// it fresh. Call once from setup, after the app and remote states.
#[cfg_attr(not(feature = "gpui-shell"), allow(dead_code))]
pub fn start(app: &AppHandle) {
  let effects = Arc::new(AppEffects {
    app: app.clone(),
    window: Mutex::new(None),
  });
  let supabase = Arc::new(Supabase::new(
    &desktop_env().supabase,
    Arc::new(DbStore(app.clone())),
  ));
  let cloud = Cloud::new(supabase, effects.clone());
  app.manage(CloudState {
    cloud: cloud.clone(),
    effects,
  });
  tauri::async_runtime::spawn(async move {
    cloud.start().await;
    cloud.keep_fresh().await;
  });
}

/// The account, while Rust owns it.
pub fn owned_cloud(app: &AppHandle) -> Option<Arc<Cloud>> {
  app.try_state::<CloudState>().map(|s| s.cloud.clone())
}

/// Trades the one-time token of a `treq://auth/callback` link, when Rust
/// owns the account. False leaves the link to the webview.
pub fn handle_auth_callback(app: &AppHandle, url: &str) -> bool {
  let Some(cloud) = owned_cloud(app) else {
    return false;
  };
  if let Some(token) = crate::core::auth::callback_token(url) {
    tauri::async_runtime::spawn(async move {
      if let Err(error) = cloud.exchange_token(&token).await {
        log::warn!("sign-in token exchange failed: {error}");
      }
    });
  }
  true
}

/// A window came to the front: reread the plan, as a purchase on the
/// website may have changed it.
#[cfg_attr(not(feature = "gpui-shell"), allow(dead_code))]
pub fn window_focused(app: &AppHandle) {
  if let Some(cloud) = owned_cloud(app) {
    tauri::async_runtime::spawn(async move { cloud.refresh_subscription().await });
  }
}

/// A cloud workspace action, by the id the account screens use.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase", tag = "action")]
pub enum CloudAction {
  Refresh,
  Provision,
  Wake,
  Repair,
  Delete,
  OpenRepositories,
  #[serde(rename_all = "camelCase")]
  Reauthenticate {
    endpoint_id: String,
  },
  #[serde(rename_all = "camelCase")]
  RevokeKey {
    key_reference: String,
  },
}

/// Runs `action`, sending any endpoint it connects to window `label`.
pub async fn run_cloud_action(
  app: &AppHandle,
  label: &str,
  action: CloudAction,
) -> Result<(), String> {
  let state = app
    .try_state::<CloudState>()
    .ok_or("The account is not available in this window.")?;
  *state.effects.window.lock_or_recover() = Some(label.to_string());
  let cloud = state.cloud.clone();
  match action {
    CloudAction::Refresh => cloud.refresh_cloud().await,
    CloudAction::Provision => cloud.provision().await,
    CloudAction::Wake => cloud.wake().await,
    CloudAction::Repair => cloud.reprovision().await,
    CloudAction::Delete => cloud.delete().await,
    CloudAction::OpenRepositories => cloud.open_repositories().await,
    CloudAction::Reauthenticate { endpoint_id } => cloud.reauthenticate(&endpoint_id).await,
    CloudAction::RevokeKey { key_reference } => cloud.revoke_key(&key_reference).await,
  }
  Ok(())
}

fn owned(app: &AppHandle) -> Result<Arc<Cloud>, String> {
  owned_cloud(app).ok_or_else(|| "The account is not available in this window.".to_string())
}

/// The account, or `None` when the webview owns it (no GPUI shell).
#[tauri::command]
pub fn cloud_account(app: AppHandle) -> Option<AccountSnapshot> {
  owned_cloud(&app).map(|cloud| cloud.snapshot())
}

#[tauri::command]
pub async fn cloud_sign_out(app: AppHandle) -> Result<(), String> {
  owned(&app)?.sign_out().await;
  Ok(())
}

#[tauri::command]
pub async fn cloud_exchange_token(app: AppHandle, token: String) -> Result<(), String> {
  owned(&app)?.exchange_token(&token).await
}

#[tauri::command]
pub async fn cloud_refresh_subscription(app: AppHandle) -> Result<(), String> {
  owned(&app)?.refresh_subscription().await;
  Ok(())
}

#[tauri::command]
pub async fn cloud_workspace_action(
  app: AppHandle,
  window: tauri::WebviewWindow,
  action: CloudAction,
) -> Result<(), String> {
  run_cloud_action(&app, window.label(), action).await
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn reads_the_actions_the_webview_sends() {
    let action: CloudAction = serde_json::from_value(serde_json::json!({
      "action": "reauthenticate", "endpointId": "e1"
    }))
    .unwrap();
    assert_eq!(
      action,
      CloudAction::Reauthenticate {
        endpoint_id: "e1".into()
      }
    );
    let action: CloudAction =
      serde_json::from_value(serde_json::json!({"action": "openRepositories"})).unwrap();
    assert_eq!(action, CloudAction::OpenRepositories);
  }
}
