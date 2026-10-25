//! The Supabase account, which Rust owns ([`crate::core::cloud`]): what the
//! Account and Integrations tabs and the remote setup dialog read of it,
//! and what they ask of it.
//!
//! The account reaches GPUI as the [`Account`] global, set on every change
//! (`gpui_shell::account_changed`); views observe it. Calls go through
//! [`AccountHost`], faked in tests.

use std::sync::Arc;

use tauri_plugin_gpui::gpui::{App, Global};

use super::settings::{
  AccountRoute, CloudEndpoint, CloudInstance, CloudRoute, Subscription, UsageReport, User,
};
pub use crate::commands::cloud::CloudAction;
use crate::{
  core::{
    account::{self, GithubRepository},
    cloud::{AccountSnapshot, Cloud, CloudWorkspace},
    supabase::ApiError,
  },
  gpui_shell::native::Backend,
};

/// The account as the screens draw it; `None` until Rust has it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Account(pub Option<AccountRoute>);

impl Global for Account {}

/// The account, or one still loading.
pub fn account(cx: &App) -> AccountRoute {
  cx.try_global::<Account>()
    .and_then(|a| a.0.clone())
    .unwrap_or(AccountRoute {
      loading: true,
      ..AccountRoute::default()
    })
}

/// What the account calls ask of the backend; faked in tests. Everything
/// but `sign_out` and `cloud_action` blocks.
pub trait AccountHost: Send + Sync {
  /// Signs out, cutting off managed SSH access first. Returns at once.
  fn sign_out(&self);
  /// Trades a pasted sign-in token for a session.
  fn exchange_token(&self, token: &str) -> Result<(), String>;
  fn github_repositories(&self) -> Result<Vec<GithubRepository>, String>;
  /// Linear's OAuth page for this account.
  fn linear_oauth_url(&self) -> Result<String, ApiError>;
  /// Deletes the account on the server, then signs out here; `Err` is the
  /// server's reason it was not deleted.
  fn delete_account(&self) -> Result<(), ApiError>;
  /// Starts a cloud workspace action; it reports through the account.
  fn cloud_action(&self, action: CloudAction);
}

pub(crate) fn cloud_route(cloud: &CloudWorkspace) -> CloudRoute {
  let status = cloud.instance_status.as_ref();
  CloudRoute {
    instance: status
      .and_then(|s| s.instance.as_ref())
      .map(|i| CloudInstance {
        status: i.status.clone(),
        provider_resource_id: i.provider_resource_id.clone(),
        disk_quota_gb: i.disk_quota_gb,
      }),
    endpoint: status
      .and_then(|s| s.endpoint_summary())
      .map(|e| CloudEndpoint {
        username: e.username,
        hostname: e.hostname,
        port: e.port,
      }),
    provisioning_stage: cloud.provisioning_stage.clone(),
    provisioning_error: cloud.provisioning_error.clone(),
    usage: cloud.usage.as_ref().map(|u| UsageReport {
      repository_count: u.repository_count as u64,
      workspace_count: u.workspace_count as u64,
      disk_used_bytes: u.disk_used_bytes,
    }),
    usage_error: cloud.usage_error.clone(),
    submitting: cloud.submitting,
  }
}

/// `useGpuiAccount`'s state, from Rust's account.
pub fn route_from(snapshot: &AccountSnapshot) -> AccountRoute {
  let auth = &snapshot.auth;
  AccountRoute {
    loading: auth.loading,
    signing_in: auth.signing_in,
    sign_in_error: auth.sign_in_error.clone(),
    user: auth.session.as_ref().map(|s| User {
      name: s.user.display_name(),
      email: s.user.email.clone(),
      avatar_url: s.user.avatar_url(),
    }),
    subscription: snapshot.subscription.as_ref().map(|s| Subscription {
      plan: s.plan.clone(),
      status: s.status.clone(),
      current_period_end: s.current_period_end.clone(),
    }),
    cloud: Some(cloud_route(&snapshot.cloud)),
    ..AccountRoute::default()
  }
}

impl Backend {
  fn cloud(&self) -> Result<Arc<Cloud>, String> {
    crate::commands::cloud::owned_cloud(self.app())
      .ok_or_else(|| "The account is not available in this window.".to_string())
  }
}

impl AccountHost for Backend {
  fn sign_out(&self) {
    if let Ok(cloud) = self.cloud() {
      tauri::async_runtime::spawn(async move { cloud.sign_out().await });
    }
  }

  fn exchange_token(&self, token: &str) -> Result<(), String> {
    let cloud = self.cloud()?;
    crate::gpui_shell::block_on(cloud.exchange_token(token))
  }

  fn github_repositories(&self) -> Result<Vec<GithubRepository>, String> {
    let cloud = self.cloud()?;
    crate::gpui_shell::block_on(account::github_repositories(&cloud.supabase))
      .map_err(|e| e.to_string())
  }

  fn linear_oauth_url(&self) -> Result<String, ApiError> {
    let cloud = self.cloud().map_err(|e| ApiError::new("unavailable", e))?;
    crate::gpui_shell::block_on(account::linear_oauth_url(&cloud.supabase))
  }

  fn delete_account(&self) -> Result<(), ApiError> {
    let cloud = self.cloud().map_err(|e| ApiError::new("unavailable", e))?;
    crate::gpui_shell::block_on(cloud.delete_account())
  }

  fn cloud_action(&self, action: CloudAction) {
    let (app, label) = (self.app().clone(), self.label().to_string());
    tauri::async_runtime::spawn(async move {
      if let Err(error) = crate::commands::cloud::run_cloud_action(&app, &label, action).await {
        log::warn!("cloud workspace action failed: {error}");
      }
    });
  }
}
