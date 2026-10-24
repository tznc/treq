//! The desktop app's Supabase account, owned by Rust: the session, the
//! subscription, and the managed cloud workspace (its status, usage and
//! connection, with certificate renewal).
//!
//! [`Cloud`] keeps one [`AccountSnapshot`] and reports every change through
//! [`CloudEffects::changed`]; GPUI draws from it and the webview mirrors it.
//! What reaches outside (the SSH pool, the relay and Linear proxy tokens,
//! the webview's endpoint) goes through [`CloudEffects`], so the flows run
//! against a fake in tests.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde::Serialize;

use super::{
  account::{self, Subscription},
  auth::{AuthStatus, Session},
  control_plane::{self, InstanceStatus},
  managed::{self, Lease, RenewalHooks},
  remote::MachineUsageReport,
  remote_control_plane::{SshAuthentication, SshEndpoint, SshEndpointSource},
  remote_local_keys::LocalSshIdentity,
  remote_ssh_transport::CutoffReason,
  supabase::{ApiError, Supabase},
};
use crate::lock_ext::LockExt;

/// The comment the desktop app registers its SSH key with.
const KEY_COMMENT: &str = "treq-managed-vm";

/// The cloud workspace card's state (`CloudWorkspaceCardProps`).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudWorkspace {
  pub instance_status: Option<InstanceStatus>,
  pub provisioning_stage: Option<String>,
  pub provisioning_error: Option<String>,
  pub usage: Option<MachineUsageReport>,
  pub usage_error: Option<String>,
  /// One of the card's actions is running.
  pub submitting: bool,
}

/// Everything the account screens draw.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountSnapshot {
  pub auth: AuthStatus,
  pub subscription: Option<Subscription>,
  pub cloud: CloudWorkspace,
}

impl AccountSnapshot {
  pub fn is_pro(&self) -> bool {
    self.subscription.as_ref().is_some_and(Subscription::is_pro)
  }
}

/// How a managed endpoint changed, for whoever dispatches over it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ManagedEndpointEvent {
  /// A fresh certificate-authenticated endpoint. `open_repositories` asks
  /// to show its repository list (connect and provision do; wake and
  /// reauthentication leave the current view alone).
  #[serde(rename_all = "camelCase")]
  Connected {
    endpoint: SshEndpoint,
    generation: u64,
    open_repositories: bool,
  },
  /// The same endpoint with a renewed certificate.
  Renewed { endpoint: SshEndpoint },
}

/// What [`Cloud`] does outside itself.
#[async_trait]
pub trait CloudEffects: Send + Sync {
  /// The account changed.
  fn changed(&self, snapshot: &AccountSnapshot);
  /// The access token to hand the SSH relay and the Linear proxy (`None`
  /// signed out). `supabase_url` is the project the token belongs to.
  fn access_token(&self, supabase_url: &str, token: Option<&str>);
  fn managed_endpoint(&self, event: ManagedEndpointEvent);
  /// Tears down connections to `endpoint_id` and records why.
  async fn cut_off(&self, endpoint_id: &str, reason: CutoffReason);
  /// Signing out: cuts off every managed endpoint.
  async fn cut_off_managed(&self);
  async fn clear_cutoff(&self, endpoint_id: &str);
  fn local_identities(&self) -> Result<Vec<LocalSshIdentity>, String>;
  fn read_public_key(&self, key_reference: &str) -> Result<String, String>;
  async fn machine_usage(&self, endpoint: &SshEndpoint) -> Result<MachineUsageReport, String>;
}

/// The cloud workspace connection Rust keeps renewed.
#[derive(Default)]
struct Managed {
  endpoint: Option<SshEndpoint>,
  key_reference: Option<String>,
  key_id: Option<String>,
  lease: Option<Lease>,
  renewal: Option<tokio::task::JoinHandle<()>>,
}

impl Managed {
  fn stop(&mut self) {
    if let Some(task) = self.renewal.take() {
      task.abort();
    }
  }
}

pub struct Cloud {
  pub supabase: Arc<Supabase>,
  effects: Arc<dyn CloudEffects>,
  subscription: Mutex<Option<Subscription>>,
  workspace: Mutex<CloudWorkspace>,
  managed: Mutex<Managed>,
  /// The user the subscription was read for.
  subscribed_user: Mutex<Option<String>>,
  last_token: Mutex<Option<Option<String>>>,
  /// [`Cloud::sign_out`] runs, and cuts managed access off itself.
  signing_out: std::sync::atomic::AtomicBool,
}

/// `pickDefaultKeyReference`: `id_ed25519` if present, else the first key.
pub fn pick_default_key_reference(identities: &[LocalSshIdentity]) -> Option<String> {
  identities
    .iter()
    .find(|i| i.label == "id_ed25519")
    .or(identities.first())
    .map(|i| i.reference.clone())
}

fn generation_of(endpoint: &SshEndpoint, fallback: u64) -> u64 {
  match endpoint.source {
    SshEndpointSource::Managed { generation, .. } => generation,
    _ => fallback,
  }
}

impl Cloud {
  pub fn new(supabase: Arc<Supabase>, effects: Arc<dyn CloudEffects>) -> Arc<Self> {
    let cloud = Arc::new(Self {
      supabase,
      effects,
      subscription: Mutex::new(None),
      workspace: Mutex::new(CloudWorkspace::default()),
      managed: Mutex::new(Managed::default()),
      subscribed_user: Mutex::new(None),
      last_token: Mutex::new(Some(None)),
      signing_out: Default::default(),
    });
    let weak = Arc::downgrade(&cloud);
    cloud.supabase.auth.subscribe(move |status| {
      if let Some(cloud) = weak.upgrade() {
        if cloud.auth_changed(status) {
          cloud.session_ended();
        }
      }
    });
    cloud
  }

  pub fn snapshot(&self) -> AccountSnapshot {
    AccountSnapshot {
      auth: self.supabase.auth.status(),
      subscription: self.subscription.lock_or_recover().clone(),
      cloud: self.workspace.lock_or_recover().clone(),
    }
  }

  fn changed(&self) {
    self.effects.changed(&self.snapshot());
  }

  fn update_workspace(&self, f: impl FnOnce(&mut CloudWorkspace)) {
    f(&mut self.workspace.lock_or_recover());
    self.changed();
  }

  /// A session that ended without [`Cloud::sign_out`] (the server refused
  /// its refresh) still ends managed access, as a sign-out does.
  fn session_ended(self: &Arc<Self>) {
    if self.signing_out.load(std::sync::atomic::Ordering::SeqCst) {
      return;
    }
    let Ok(runtime) = tokio::runtime::Handle::try_current() else {
      return;
    };
    let cloud = self.clone();
    runtime.spawn(async move {
      let stopped = std::mem::take(&mut *cloud.managed.lock_or_recover());
      drop(stopped);
      cloud.effects.cut_off_managed().await;
    });
  }

  /// Pushes a new access token, and forgets the subscription of a user
  /// who signed out. True when a session just ended.
  fn auth_changed(&self, status: &AuthStatus) -> bool {
    let token = status.session.as_ref().map(|s| s.access_token.clone());
    let (pushed, ended) = {
      let mut last = self.last_token.lock_or_recover();
      let fresh = last.as_ref() != Some(&token);
      let ended = matches!(&*last, Some(Some(_))) && token.is_none();
      *last = Some(token.clone());
      (fresh, ended)
    };
    if pushed {
      self
        .effects
        .access_token(self.supabase.url(), token.as_deref());
    }
    if status.session.is_none() {
      *self.subscription.lock_or_recover() = None;
      *self.subscribed_user.lock_or_recover() = None;
    }
    self.changed();
    ended
  }

  /// Restores the stored session and reads its subscription. Call once at
  /// launch, then [`Cloud::keep_fresh`].
  pub async fn start(&self) {
    let _ = self.supabase.auth.restore().await;
    self.after_sign_in().await;
  }

  /// Refreshes the session near expiry, so every consumer's token stays
  /// valid (`REFRESH_CHECK_INTERVAL_MS`). Runs until aborted.
  pub async fn keep_fresh(&self) {
    loop {
      tokio::time::sleep(std::time::Duration::from_secs(60)).await;
      let _ = self.supabase.auth.access_token().await;
    }
  }

  async fn after_sign_in(&self) {
    let user = self.supabase.auth.session().map(|s: Session| s.user.id);
    let first = *self.subscribed_user.lock_or_recover() != user;
    if user.is_some() && first {
      *self.subscribed_user.lock_or_recover() = user;
      self.refresh_subscription().await;
    }
  }

  pub async fn exchange_token(&self, token: &str) -> Result<(), String> {
    self.supabase.auth.exchange_token(token).await?;
    self.after_sign_in().await;
    Ok(())
  }

  /// Signing out ends the authorization behind managed SSH access, so open
  /// managed connections and terminals are cut off, not left running.
  pub async fn sign_out(&self) {
    self.end_session(true).await;
  }

  /// Deletes the signed-in account (the `delete-account` Edge Function),
  /// then ends the session here as signing out does. The user no longer
  /// exists, so the session is forgotten locally, without a logout call.
  /// A refusal (last owner of an organization, a failed cancellation)
  /// keeps the session and returns the server's reason.
  pub async fn delete_account(&self) -> Result<(), ApiError> {
    self
      .supabase
      .invoke::<serde_json::Value>("delete-account", &serde_json::json!({}))
      .await?;
    self.end_session(false).await;
    Ok(())
  }

  async fn end_session(&self, on_server: bool) {
    let stopped = {
      let mut managed = self.managed.lock_or_recover();
      managed.stop();
      std::mem::take(&mut *managed)
    };
    drop(stopped);
    self.effects.cut_off_managed().await;
    let flag = std::sync::atomic::Ordering::SeqCst;
    self.signing_out.store(true, flag);
    if on_server {
      self.supabase.auth.sign_out().await;
    } else {
      self.supabase.auth.forget_session();
    }
    self.signing_out.store(false, flag);
    *self.workspace.lock_or_recover() = CloudWorkspace::default();
    self.changed();
  }

  /// Rereads the subscription (on window focus, after a purchase on the
  /// website). A failed read keeps the last one.
  pub async fn refresh_subscription(&self) {
    if self.supabase.auth.session().is_none() {
      return;
    }
    match account::subscription(&self.supabase).await {
      Ok(subscription) => {
        *self.subscription.lock_or_recover() = subscription;
        self.changed();
      }
      Err(error) => log::warn!("reading the subscription failed: {error}"),
    }
  }

  // -- The cloud workspace ------------------------------------------------

  /// Rereads the instance status and, once it is ready, its usage.
  pub async fn refresh_cloud(&self) {
    self.update_workspace(|w| w.provisioning_error = None);
    if let Ok(status) = control_plane::instance_status(&self.supabase).await {
      self.update_workspace(|w| w.instance_status = Some(status));
    }
    self.load_usage().await;
  }

  async fn refresh_status(&self) {
    if let Ok(status) = control_plane::instance_status(&self.supabase).await {
      self.update_workspace(|w| w.instance_status = Some(status));
    }
  }

  /// The endpoint usage is read over: this device's certificate endpoint
  /// for the instance when there is one, else the server's.
  fn usage_endpoint(&self, status: &InstanceStatus) -> Option<SshEndpoint> {
    let summary = status.endpoint_summary()?;
    let managed = self.managed.lock_or_recover();
    managed
      .endpoint
      .clone()
      .filter(|e| {
        e.id == summary.id
          && matches!(
            &e.authentication,
            SshAuthentication::Certificate {
              certificate: Some(_),
              ..
            }
          )
      })
      .or_else(|| serde_json::from_value(status.endpoint.clone()?).ok())
  }

  /// Usage is computed on the machine itself, so it is read once the cloud
  /// workspace is ready.
  async fn load_usage(&self) {
    let status = self.workspace.lock_or_recover().instance_status.clone();
    let Some(status) = status.filter(|s| s.instance.as_ref().is_some_and(|i| i.status == "ready"))
    else {
      return;
    };
    let Some(endpoint) = self.usage_endpoint(&status) else {
      return;
    };
    self.update_workspace(|w| {
      w.usage = None;
      w.usage_error = None;
    });
    let result = self.effects.machine_usage(&endpoint).await;
    self.update_workspace(|w| match result {
      Ok(usage) => w.usage = Some(usage),
      Err(error) => w.usage_error = Some(error),
    });
  }

  fn instance_id(&self) -> Option<String> {
    self
      .workspace
      .lock_or_recover()
      .instance_status
      .as_ref()
      .and_then(|s| s.instance.as_ref())
      .map(|i| i.instance_id.clone())
  }

  /// Runs a card action with `submitting` set.
  async fn submitting<F: std::future::Future<Output = ()>>(&self, run: F) {
    self.update_workspace(|w| w.submitting = true);
    run.await;
    self.update_workspace(|w| w.submitting = false);
  }

  fn fail(&self, error: impl Into<String>) {
    let error = error.into();
    self.update_workspace(|w| w.provisioning_error = Some(error));
  }

  fn key_reference(&self) -> Result<String, String> {
    if let Some(reference) = self.managed.lock_or_recover().key_reference.clone() {
      return Ok(reference);
    }
    pick_default_key_reference(&self.effects.local_identities()?).ok_or_else(|| {
      "No SSH key found in ~/.ssh. Create one with `ssh-keygen -t ed25519`, then try again.".into()
    })
  }

  async fn register_key(&self, key_reference: &str) -> Result<String, String> {
    let public_key = self.effects.read_public_key(key_reference)?;
    let key = control_plane::register_client_key(
      &self.supabase,
      &public_key,
      KEY_COMMENT,
      &format!("register-client-key:{key_reference}"),
    )
    .await
    .map_err(|e| e.to_string())?;
    self.managed.lock_or_recover().key_id = Some(key.id.clone());
    Ok(key.id)
  }

  /// Issues a certificate and makes its endpoint the active one, renewed
  /// until a cutoff.
  async fn issue_and_activate(
    self: &Arc<Self>,
    instance_id: &str,
    key_reference: &str,
    open_repositories: bool,
  ) -> Result<SshEndpoint, String> {
    let key_id = self.register_key(key_reference).await?;
    let (lease, endpoint) =
      managed::issue(&self.supabase, instance_id, &key_id, key_reference).await?;
    {
      let mut managed = self.managed.lock_or_recover();
      managed.stop();
      managed.endpoint = Some(endpoint.clone());
      managed.key_reference = Some(key_reference.to_string());
      managed.lease = Some(lease.clone());
      managed.renewal = Some(tokio::spawn(managed::renewal_loop(
        self.supabase.clone(),
        lease,
        key_reference.to_string(),
        Arc::new(Hooks(Arc::downgrade(self))),
      )));
    }
    self
      .effects
      .managed_endpoint(ManagedEndpointEvent::Connected {
        generation: generation_of(&endpoint, 0),
        endpoint: endpoint.clone(),
        open_repositories,
      });
    self.refresh_status().await;
    Ok(endpoint)
  }

  fn wait_progress(&self) -> impl Fn(&InstanceStatus) + Send + Sync + '_ {
    move |status: &InstanceStatus| {
      let state = status
        .instance
        .as_ref()
        .map_or("unprovisioned", |i| i.status.as_str())
        .to_string();
      let status = status.clone();
      self.update_workspace(|w| {
        w.instance_status = Some(status);
        w.provisioning_stage = Some(format!("Waiting for the cloud workspace ({state})..."));
      });
    }
  }

  /// "Create cloud workspace": registration, provisioning, readiness,
  /// certificate, then the repository list.
  pub async fn provision(self: &Arc<Self>) {
    self.update_workspace(|w| {
      w.provisioning_error = None;
      w.provisioning_stage = Some("Requesting provisioning...".into());
    });
    let result = async {
      let key_reference = self.key_reference()?;
      self.register_key(&key_reference).await?;
      // One idempotency key per click, so a retry after a failure is not a
      // replay of that failure.
      let key = format!("provision-managed-{}", uuid::Uuid::new_v4());
      control_plane::ensure(&self.supabase, &key)
        .await
        .map_err(|e| e.to_string())?;
      let progress = self.wait_progress();
      let status = managed::wait_for_ready(
        &self.supabase,
        managed::READY_POLL,
        managed::READY_TIMEOUT,
        &progress,
      )
      .await?;
      let instance_id = status
        .instance
        .map(|i| i.instance_id)
        .ok_or("Managed instance became ready without an instance id.")?;
      self
        .issue_and_activate(&instance_id, &key_reference, true)
        .await
        .map(drop)
    }
    .await;
    if let Err(error) = result {
      self.fail(error);
      // The control plane records the failure; reload so the card offers
      // a retry instead of a fresh create.
      self.refresh_status().await;
    }
    self.update_workspace(|w| w.provisioning_stage = None);
  }

  /// Wake, wait for readiness, then a fresh certificate.
  pub async fn wake(self: &Arc<Self>) {
    let Some(instance_id) = self.instance_id() else {
      return;
    };
    self
      .submitting(async {
        self.update_workspace(|w| w.provisioning_error = None);
        let result = async {
          let key_reference = self.key_reference()?;
          let key =
            control_plane::wake_idempotency_key(&instance_id, &uuid::Uuid::new_v4().to_string());
          control_plane::wake(&self.supabase, &instance_id, &key)
            .await
            .map_err(|e| e.to_string())?;
          let status = managed::wait_for_ready(
            &self.supabase,
            managed::READY_POLL,
            managed::READY_TIMEOUT,
            &|status| {
              let status = status.clone();
              self.update_workspace(|w| w.instance_status = Some(status));
            },
          )
          .await?;
          let instance_id = status
            .instance
            .map(|i| i.instance_id)
            .unwrap_or(instance_id);
          self
            .issue_and_activate(&instance_id, &key_reference, false)
            .await
            .map(drop)
        }
        .await;
        if let Err(error) = result {
          self.fail(error);
        }
      })
      .await;
  }

  /// "Open repositories": reuses this device's endpoint for the instance,
  /// or connects to the ready instance first.
  pub async fn open_repositories(self: &Arc<Self>) {
    let status = self.workspace.lock_or_recover().instance_status.clone();
    let Some(summary) = status.as_ref().and_then(InstanceStatus::endpoint_summary) else {
      return;
    };
    let active = self
      .managed
      .lock_or_recover()
      .endpoint
      .clone()
      .filter(|e| e.id == summary.id);
    if let Some(endpoint) = active {
      let fallback = status.and_then(|s| s.instance).map_or(0, |i| i.generation);
      self
        .effects
        .managed_endpoint(ManagedEndpointEvent::Connected {
          generation: generation_of(&endpoint, fallback),
          endpoint,
          open_repositories: true,
        });
      return;
    }
    self
      .submitting(async {
        self.update_workspace(|w| w.provisioning_error = None);
        let result = async {
          let key_reference = self.key_reference()?;
          // A fresh status, so a workspace that went to sleep fails with
          // "not ready" instead of a certificate for an unreachable host.
          let status = control_plane::instance_status(&self.supabase)
            .await
            .map_err(|e| e.to_string())?;
          self.update_workspace(|w| w.instance_status = Some(status.clone()));
          let instance = status
            .instance
            .clone()
            .filter(|i| i.status == "ready")
            .ok_or_else(|| {
              format!(
                "Managed instance is not ready (status: {}).",
                status
                  .instance
                  .as_ref()
                  .map_or("unprovisioned", |i| i.status.as_str())
              )
            })?;
          self
            .issue_and_activate(&instance.instance_id, &key_reference, true)
            .await
            .map(drop)
        }
        .await;
        if let Err(error) = result {
          self.fail(error);
        }
      })
      .await;
  }

  /// A fresh certificate after a cutoff; lifts the cutoff once it is issued.
  pub async fn reauthenticate(self: &Arc<Self>, endpoint_id: &str) {
    let Some(instance_id) = self.instance_id() else {
      return;
    };
    self.update_workspace(|w| w.provisioning_error = None);
    let result = async {
      let key_reference = self.key_reference()?;
      self
        .issue_and_activate(&instance_id, &key_reference, false)
        .await?;
      self.effects.clear_cutoff(endpoint_id).await;
      Ok::<_, String>(())
    }
    .await;
    if let Err(error) = result {
      self.fail(error);
    }
  }

  pub async fn reprovision(&self) {
    let Some(instance_id) = self.instance_id() else {
      return;
    };
    self
      .submitting(async {
        self.update_workspace(|w| w.provisioning_error = None);
        let key = format!(
          "reprovision-{instance_id}-{}",
          chrono::Utc::now().timestamp_millis()
        );
        match control_plane::reprovision(&self.supabase, &instance_id, &key).await {
          Ok(_) => self.refresh_status().await,
          Err(error) => self.fail(error.to_string()),
        }
      })
      .await;
  }

  pub async fn delete(&self) {
    let Some(instance_id) = self.instance_id() else {
      return;
    };
    self
      .submitting(async {
        self.update_workspace(|w| w.provisioning_error = None);
        let key = format!(
          "delete-{instance_id}-{}",
          chrono::Utc::now().timestamp_millis()
        );
        match control_plane::delete(&self.supabase, &instance_id, &key).await {
          Ok(_) => self.refresh_status().await,
          Err(error) => self.fail(error.to_string()),
        }
      })
      .await;
  }

  /// Revoking the active key cuts the active endpoint off at once, as the
  /// next renewal would.
  pub async fn revoke_key(&self, key_reference: &str) {
    self.update_workspace(|w| w.provisioning_error = None);
    let (key_id, endpoint_id) = {
      let managed = self.managed.lock_or_recover();
      (
        managed.key_id.clone(),
        managed.endpoint.as_ref().map(|e| e.id.clone()),
      )
    };
    let id = key_id.clone().unwrap_or_else(|| key_reference.to_string());
    match control_plane::revoke_client_key(&self.supabase, &id, &format!("revoke-{id}")).await {
      Ok(_) => {
        if let (Some(_), Some(endpoint_id)) = (key_id, endpoint_id) {
          self.managed.lock_or_recover().stop();
          self
            .effects
            .cut_off(&endpoint_id, CutoffReason::KeyRevoked)
            .await;
        }
      }
      Err(error) => self.fail(error.to_string()),
    }
  }
}

/// The renewal loop's effects on [`Cloud`].
struct Hooks(std::sync::Weak<Cloud>);

#[async_trait]
impl RenewalHooks for Hooks {
  fn renewed(&self, lease: &Lease, endpoint: SshEndpoint) {
    let Some(cloud) = self.0.upgrade() else {
      return;
    };
    {
      let mut managed = cloud.managed.lock_or_recover();
      // The user may have switched to another endpoint since.
      if managed.endpoint.as_ref().map(|e| &e.id) != Some(&endpoint.id) {
        return;
      }
      managed.endpoint = Some(endpoint.clone());
      managed.lease = Some(lease.clone());
    }
    cloud
      .effects
      .managed_endpoint(ManagedEndpointEvent::Renewed { endpoint });
  }

  async fn cut_off(&self, lease: &Lease, reason: CutoffReason) {
    if let Some(cloud) = self.0.upgrade() {
      cloud.effects.cut_off(&lease.endpoint_id, reason).await;
    }
  }
}

#[cfg(test)]
mod tests {
  use serde_json::{json, Value};
  use wiremock::{
    matchers::{body_partial_json, header, method, path},
    Mock, MockServer, ResponseTemplate,
  };

  use super::*;
  use crate::core::supabase::test_support::client;

  #[derive(Default)]
  struct Fake {
    snapshots: Mutex<Vec<AccountSnapshot>>,
    tokens: Mutex<Vec<Option<String>>>,
    endpoints: Mutex<Vec<ManagedEndpointEvent>>,
    log: Mutex<Vec<String>>,
  }

  #[async_trait]
  impl CloudEffects for Fake {
    fn changed(&self, snapshot: &AccountSnapshot) {
      self.snapshots.lock().unwrap().push(snapshot.clone());
    }
    fn access_token(&self, _: &str, token: Option<&str>) {
      self.tokens.lock().unwrap().push(token.map(str::to_string));
    }
    fn managed_endpoint(&self, event: ManagedEndpointEvent) {
      self.endpoints.lock().unwrap().push(event);
    }
    async fn cut_off(&self, endpoint_id: &str, reason: CutoffReason) {
      self.log.lock().unwrap().push(format!(
        "cut off {endpoint_id} {}",
        managed::reason_code(reason)
      ));
    }
    async fn cut_off_managed(&self) {
      self.log.lock().unwrap().push("cut off managed".into());
    }
    async fn clear_cutoff(&self, endpoint_id: &str) {
      self
        .log
        .lock()
        .unwrap()
        .push(format!("clear {endpoint_id}"));
    }
    fn local_identities(&self) -> Result<Vec<LocalSshIdentity>, String> {
      Ok(vec![
        identity("id_rsa", "/k/id_rsa"),
        identity("id_ed25519", "/k/id_ed25519"),
      ])
    }
    fn read_public_key(&self, key_reference: &str) -> Result<String, String> {
      Ok(format!("ssh-ed25519 AAA {key_reference}"))
    }
    async fn machine_usage(&self, endpoint: &SshEndpoint) -> Result<MachineUsageReport, String> {
      self
        .log
        .lock()
        .unwrap()
        .push(format!("usage {}", endpoint.id));
      Ok(MachineUsageReport {
        repository_count: 2,
        workspace_count: 3,
        disk_used_bytes: 10,
      })
    }
  }

  fn identity(label: &str, reference: &str) -> LocalSshIdentity {
    LocalSshIdentity {
      reference: reference.into(),
      label: label.into(),
      fingerprint_sha256: "SHA256:x".into(),
      algorithm: "ed25519".into(),
    }
  }

  fn tokens(access: &str) -> Value {
    json!({
      "access_token": access, "refresh_token": "r", "expires_in": 3600,
      "user": {"id": "u1", "email": "ada@example.com"}
    })
  }

  fn endpoint() -> Value {
    json!({
      "id": "e1", "instance_id": "i1",
      "source": {"type": "managed", "provider": "fly_sprites", "generation": 4},
      "hostname": "h", "port": 22, "username": "treq", "host_keys": [],
      "authentication": {"type": "public_key", "key_reference": "server-key"}
    })
  }

  fn status(state: &str) -> Value {
    json!({
      "instance": {"instance_id": "i1", "status": state, "generation": 4, "provider_resource_id": "p", "disk_quota_gb": 10},
      "endpoint": endpoint()
    })
  }

  async fn mount(server: &MockServer, function: &str, body: Value, response: Value) {
    Mock::given(method("POST"))
      .and(path(format!("/functions/v1/{function}")))
      .and(body_partial_json(body))
      .respond_with(ResponseTemplate::new(200).set_body_json(response))
      .mount(server)
      .await;
  }

  async fn signed_in(server: &MockServer) -> (Arc<Cloud>, Arc<Fake>) {
    mount(server, "exchange-desktop-token", json!({}), tokens("a1")).await;
    Mock::given(method("GET"))
      .and(path("/rest/v1/subscriptions"))
      .respond_with(ResponseTemplate::new(200).set_body_json(json!([
        {"status": "active", "plan": "pro", "current_period_end": null}
      ])))
      .mount(server)
      .await;
    let (supabase, _) = client(&server.uri());
    let fake = Arc::new(Fake::default());
    let cloud = Cloud::new(Arc::new(supabase), fake.clone());
    cloud.exchange_token("t").await.unwrap();
    (cloud, fake)
  }

  #[test]
  fn prefers_the_ed25519_key() {
    let fake = Fake::default();
    let ids = fake.local_identities().unwrap();
    assert_eq!(
      pick_default_key_reference(&ids).as_deref(),
      Some("/k/id_ed25519")
    );
    assert_eq!(
      pick_default_key_reference(&ids[..1]).as_deref(),
      Some("/k/id_rsa")
    );
    assert_eq!(pick_default_key_reference(&[]), None);
  }

  #[tokio::test]
  async fn signing_in_pushes_the_token_and_reads_the_subscription() {
    let server = MockServer::start().await;
    let (cloud, fake) = signed_in(&server).await;
    assert_eq!(*fake.tokens.lock().unwrap(), vec![Some("a1".to_string())]);
    let last = fake.snapshots.lock().unwrap().last().cloned().unwrap();
    assert!(last.is_pro());
    assert_eq!(last, cloud.snapshot());
    assert_eq!(
      last.auth.session.unwrap().user.email.as_deref(),
      Some("ada@example.com")
    );
  }

  #[tokio::test]
  async fn signing_out_cuts_off_managed_access_and_clears_the_plan() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
      .and(path("/auth/v1/logout"))
      .respond_with(ResponseTemplate::new(204))
      .mount(&server)
      .await;
    let (cloud, fake) = signed_in(&server).await;
    cloud.sign_out().await;
    assert_eq!(
      *fake.log.lock().unwrap(),
      vec!["cut off managed".to_string()]
    );
    assert_eq!(fake.tokens.lock().unwrap().last().cloned(), Some(None));
    let snapshot = cloud.snapshot();
    assert_eq!(snapshot.subscription, None);
    assert_eq!(snapshot.auth.session, None);
  }

  #[tokio::test]
  async fn deleting_the_account_cuts_off_access_and_forgets_the_session_locally() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
      .and(path("/functions/v1/delete-account"))
      .and(header("authorization", "Bearer a1"))
      .respond_with(ResponseTemplate::new(200).set_body_json(json!({"deleted": true})))
      .expect(1)
      .mount(&server)
      .await;
    // The user no longer exists, so there is no server session to end.
    Mock::given(method("POST"))
      .and(path("/auth/v1/logout"))
      .respond_with(ResponseTemplate::new(204))
      .expect(0)
      .mount(&server)
      .await;
    let (cloud, fake) = signed_in(&server).await;
    cloud.delete_account().await.unwrap();
    assert_eq!(
      *fake.log.lock().unwrap(),
      vec!["cut off managed".to_string()]
    );
    let snapshot = cloud.snapshot();
    assert_eq!(snapshot.auth.session, None);
    assert_eq!(snapshot.subscription, None);
  }

  #[tokio::test]
  async fn a_refused_account_deletion_keeps_the_session() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
      .and(path("/functions/v1/delete-account"))
      .respond_with(ResponseTemplate::new(409).set_body_json(json!({
        "error": "You are the last owner of Acme. Make another member an owner, or delete the organization, before deleting your account.",
        "code": "last_owner",
      })))
      .mount(&server)
      .await;
    let (cloud, fake) = signed_in(&server).await;
    let error = cloud.delete_account().await.unwrap_err();
    assert_eq!(error.status, 409);
    assert_eq!(error.code, "last_owner");
    assert!(error.message.starts_with("You are the last owner of Acme."));
    assert!(fake.log.lock().unwrap().is_empty());
    assert!(cloud.snapshot().auth.session.is_some());
  }

  #[tokio::test]
  async fn a_session_the_server_ends_cuts_off_managed_access() {
    let server = MockServer::start().await;
    mount(
      &server,
      "exchange-desktop-token",
      json!({}),
      json!({
        "access_token": "a1", "refresh_token": "r", "expires_in": 10,
        "user": {"id": "u1", "email": "ada@example.com"}
      }),
    )
    .await;
    Mock::given(method("POST"))
      .and(path("/auth/v1/token"))
      .respond_with(ResponseTemplate::new(400).set_body_json(json!({"error": "invalid_grant"})))
      .mount(&server)
      .await;
    let (supabase, _) = client(&server.uri());
    let fake = Arc::new(Fake::default());
    let cloud = Cloud::new(Arc::new(supabase), fake.clone());
    // Reading the plan refreshes the near-expired session, which the
    // server refuses.
    cloud.exchange_token("t").await.unwrap();
    let _ = cloud.supabase.auth.access_token().await;
    assert_eq!(cloud.snapshot().auth.session, None);
    for _ in 0..50 {
      if !fake.log.lock().unwrap().is_empty() {
        break;
      }
      tokio::task::yield_now().await;
    }
    assert_eq!(
      *fake.log.lock().unwrap(),
      vec!["cut off managed".to_string()]
    );
  }

  #[tokio::test]
  async fn opening_repositories_connects_with_the_local_key_and_reads_usage() {
    let server = MockServer::start().await;
    let (cloud, fake) = signed_in(&server).await;
    mount(
      &server,
      "remote-instance",
      json!({"action": "status"}),
      status("ready"),
    )
    .await;
    mount(
      &server,
      "remote-ssh-trust",
      json!({"action": "register_client_key", "comment": "treq-managed-vm", "idempotency_key": "register-client-key:/k/id_ed25519"}),
      json!({"key": {"id": "k1", "comment": "treq-managed-vm"}}),
    )
    .await;
    mount(
      &server,
      "remote-ssh-trust",
      json!({"action": "issue_certificate", "instance_id": "i1", "key_id": "k1"}),
      json!({"certificate": "cert", "serial": "1", "expires_at": "2099-01-01T00:00:00Z", "endpoint": endpoint()}),
    )
    .await;

    cloud.refresh_cloud().await;
    assert_eq!(
      cloud
        .snapshot()
        .cloud
        .instance_status
        .unwrap()
        .instance
        .unwrap()
        .status,
      "ready"
    );
    cloud.open_repositories().await;

    let events = fake.endpoints.lock().unwrap().clone();
    let [ManagedEndpointEvent::Connected {
      endpoint,
      generation,
      open_repositories,
    }] = events.as_slice()
    else {
      panic!("one connection: {events:?}");
    };
    assert_eq!((*generation, *open_repositories), (4, true));
    assert_eq!(
      endpoint.authentication,
      SshAuthentication::Certificate {
        key_reference: "/k/id_ed25519".into(),
        certificate: Some("cert".into())
      }
    );
    assert!(!cloud.snapshot().cloud.submitting);

    // Usage now goes over this device's certificate endpoint.
    cloud.refresh_cloud().await;
    assert_eq!(cloud.snapshot().cloud.usage.unwrap().workspace_count, 3);
    // A second Open repositories reuses the endpoint.
    cloud.open_repositories().await;
    assert_eq!(fake.endpoints.lock().unwrap().len(), 2);
  }

  #[tokio::test]
  async fn a_failed_action_shows_its_error() {
    let server = MockServer::start().await;
    let (cloud, _) = signed_in(&server).await;
    mount(
      &server,
      "remote-instance",
      json!({"action": "status"}),
      status("degraded"),
    )
    .await;
    Mock::given(method("POST"))
      .and(path("/functions/v1/remote-instance"))
      .and(body_partial_json(json!({"action": "reprovision"})))
      .respond_with(
        ResponseTemplate::new(402)
          .set_body_json(json!({"error": "Needs Pro", "code": "pro_required"})),
      )
      .mount(&server)
      .await;
    cloud.refresh_cloud().await;
    cloud.reprovision().await;
    let workspace = cloud.snapshot().cloud;
    assert_eq!(
      workspace.provisioning_error.as_deref(),
      Some("[pro_required] Needs Pro\nHTTP 402")
    );
    assert!(!workspace.submitting);
  }

  // -- `managed-ssh-connection.test.ts` -----------------------------------

  async fn trust_mocks(server: &MockServer, certificate: Option<u16>) {
    mount(
      server,
      "remote-ssh-trust",
      json!({"action": "register_client_key"}),
      json!({"key": {"id": "k1", "comment": "treq-managed-vm"}}),
    )
    .await;
    let response = match certificate {
      None => ResponseTemplate::new(200).set_body_json(json!({
        "certificate": "cert", "serial": "1", "expires_at": "2099-01-01T00:00:00Z", "endpoint": endpoint()
      })),
      Some(status) => ResponseTemplate::new(status).set_body_json(json!({"error": "refused"})),
    };
    Mock::given(method("POST"))
      .and(path("/functions/v1/remote-ssh-trust"))
      .and(body_partial_json(json!({"action": "issue_certificate"})))
      .respond_with(response)
      .mount(server)
      .await;
  }

  async fn bodies(server: &MockServer, action: &str) -> Vec<Value> {
    server
      .received_requests()
      .await
      .unwrap_or_default()
      .into_iter()
      .filter_map(|r| serde_json::from_slice::<Value>(&r.body).ok())
      .filter(|b| b["action"] == action)
      .collect()
  }

  /// The cutoff effects, without the usage reads beside them.
  fn cutoff_log(fake: &Fake) -> Vec<String> {
    let log = fake.log.lock().unwrap();
    log
      .iter()
      .filter(|e| !e.starts_with("usage"))
      .cloned()
      .collect()
  }

  fn connected(fake: &Fake) -> Vec<(u64, bool)> {
    fake
      .endpoints
      .lock()
      .unwrap()
      .iter()
      .filter_map(|e| match e {
        ManagedEndpointEvent::Connected {
          generation,
          open_repositories,
          ..
        } => Some((*generation, *open_repositories)),
        _ => None,
      })
      .collect()
  }

  #[tokio::test]
  async fn registers_the_public_key_with_a_stable_idempotency_key() {
    let server = MockServer::start().await;
    let (cloud, _) = signed_in(&server).await;
    mount(
      &server,
      "remote-instance",
      json!({"action": "status"}),
      status("ready"),
    )
    .await;
    trust_mocks(&server, None).await;
    cloud.refresh_cloud().await;
    cloud.open_repositories().await;
    cloud.reauthenticate("e1").await;
    let registered = bodies(&server, "register_client_key").await;
    assert_eq!(registered.len(), 2);
    for body in registered {
      // Only the public key is ever read and sent.
      assert_eq!(body["public_key"], "ssh-ed25519 AAA /k/id_ed25519");
      assert_eq!(body["idempotency_key"], "register-client-key:/k/id_ed25519");
    }
  }

  #[tokio::test]
  async fn provisions_then_polls_readiness_until_connectable() {
    let server = MockServer::start().await;
    let (cloud, fake) = signed_in(&server).await;
    mount(
      &server,
      "remote-instance",
      json!({"action": "ensure"}),
      json!({}),
    )
    .await;
    Mock::given(method("POST"))
      .and(path("/functions/v1/remote-instance"))
      .and(body_partial_json(json!({"action": "status"})))
      .respond_with(ResponseTemplate::new(200).set_body_json(status("provisioning")))
      .up_to_n_times(1)
      .with_priority(1)
      .mount(&server)
      .await;
    mount(
      &server,
      "remote-instance",
      json!({"action": "status"}),
      status("ready"),
    )
    .await;
    trust_mocks(&server, None).await;
    cloud.provision().await;
    assert!(fake
      .snapshots
      .lock()
      .unwrap()
      .iter()
      .filter_map(|s| s.cloud.provisioning_stage.clone())
      .any(|x| x == "Waiting for the cloud workspace (provisioning)..."));
    assert_eq!(connected(&fake), [(4, true)]);
    let ensured = bodies(&server, "ensure").await;
    assert!(ensured[0]["idempotency_key"]
      .as_str()
      .unwrap()
      .starts_with("provision-managed-"));
    let workspace = cloud.snapshot().cloud;
    assert_eq!(workspace.provisioning_stage, None);
    assert_eq!(workspace.provisioning_error, None);
  }

  #[tokio::test]
  async fn surfaces_a_readiness_failure_instead_of_polling_forever() {
    let server = MockServer::start().await;
    let (cloud, fake) = signed_in(&server).await;
    mount(
      &server,
      "remote-instance",
      json!({"action": "ensure"}),
      json!({}),
    )
    .await;
    mount(
      &server,
      "remote-instance",
      json!({"action": "status"}),
      status("failed"),
    )
    .await;
    trust_mocks(&server, None).await;
    cloud.provision().await;
    assert_eq!(
      cloud.snapshot().cloud.provisioning_error.as_deref(),
      Some("Managed instance provisioning failed (status: failed).")
    );
    assert!(connected(&fake).is_empty());
  }

  #[tokio::test]
  async fn opening_repositories_rejects_an_instance_that_is_not_ready() {
    let server = MockServer::start().await;
    let (cloud, fake) = signed_in(&server).await;
    mount(
      &server,
      "remote-instance",
      json!({"action": "status"}),
      status("suspended"),
    )
    .await;
    trust_mocks(&server, None).await;
    cloud.refresh_cloud().await;
    cloud.open_repositories().await;
    assert_eq!(
      cloud.snapshot().cloud.provisioning_error.as_deref(),
      Some("Managed instance is not ready (status: suspended).")
    );
    assert!(connected(&fake).is_empty());
    assert!(bodies(&server, "issue_certificate").await.is_empty());
  }

  #[tokio::test]
  async fn reauthenticating_clears_the_cutoff_only_after_a_fresh_certificate() {
    let server = MockServer::start().await;
    let (cloud, fake) = signed_in(&server).await;
    mount(
      &server,
      "remote-instance",
      json!({"action": "status"}),
      status("ready"),
    )
    .await;
    trust_mocks(&server, None).await;
    cloud.refresh_cloud().await;
    cloud.reauthenticate("e1").await;
    assert_eq!(cutoff_log(&fake), ["clear e1"]);
    assert_eq!(connected(&fake), [(4, false)]);
  }

  #[tokio::test]
  async fn a_failed_reauthentication_keeps_the_cutoff() {
    let server = MockServer::start().await;
    let (cloud, fake) = signed_in(&server).await;
    mount(
      &server,
      "remote-instance",
      json!({"action": "status"}),
      status("ready"),
    )
    .await;
    trust_mocks(&server, Some(409)).await;
    cloud.refresh_cloud().await;
    cloud.reauthenticate("e1").await;
    assert!(cutoff_log(&fake).is_empty());
    assert!(cloud.snapshot().cloud.provisioning_error.is_some());
  }

  #[tokio::test]
  async fn waking_uses_a_new_idempotency_key_every_attempt_then_reconnects() {
    let server = MockServer::start().await;
    let (cloud, fake) = signed_in(&server).await;
    mount(
      &server,
      "remote-instance",
      json!({"action": "status"}),
      status("ready"),
    )
    .await;
    mount(
      &server,
      "remote-instance",
      json!({"action": "wake"}),
      json!({}),
    )
    .await;
    trust_mocks(&server, None).await;
    cloud.refresh_cloud().await;
    cloud.wake().await;
    cloud.wake().await;
    let keys: Vec<_> = bodies(&server, "wake")
      .await
      .into_iter()
      .map(|b| b["idempotency_key"].as_str().unwrap().to_string())
      .collect();
    assert_eq!(keys.len(), 2);
    assert!(keys[0].starts_with("wake-i1-"));
    assert_ne!(keys[0], keys[1]);
    // Wake leaves the repository view alone.
    assert_eq!(connected(&fake), [(4, false), (4, false)]);
  }

  #[tokio::test]
  async fn a_renewal_swaps_the_certificate_without_reactivating_the_endpoint() {
    let server = MockServer::start().await;
    let (cloud, fake) = signed_in(&server).await;
    mount(
      &server,
      "remote-instance",
      json!({"action": "status"}),
      status("ready"),
    )
    .await;
    trust_mocks(&server, None).await;
    cloud.refresh_cloud().await;
    cloud.open_repositories().await;
    let lease = cloud.managed.lock().unwrap().lease.clone().unwrap();
    let mut renewed: SshEndpoint = serde_json::from_value(endpoint()).unwrap();
    renewed.authentication = SshAuthentication::Certificate {
      key_reference: "/k/id_ed25519".into(),
      certificate: Some("cert-2".into()),
    };
    let hooks = Hooks(Arc::downgrade(&cloud));
    hooks.renewed(&lease, renewed.clone());
    let mut other = renewed.clone();
    other.id = "e-other".into();
    hooks.renewed(&lease, other);
    let events = fake.endpoints.lock().unwrap().clone();
    assert_eq!(connected(&fake).len(), 1, "no second activation");
    assert_eq!(
      events.last(),
      Some(&ManagedEndpointEvent::Renewed { endpoint: renewed })
    );
    assert_eq!(events.len(), 2, "a renewal for another endpoint is dropped");
  }
}
