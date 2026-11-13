//! The Account and Integrations tabs (`AccountSettings`,
//! `GitHubIntegrationSettings`, `LinearIntegrationSettings`).
//!
//! The Supabase account is Rust's ([`account_data`]): the tabs draw the
//! [`Account`] global and call the backend to sign out, trade a pasted
//! sign-in token, list the GitHub repositories, start Linear OAuth and run
//! the cloud workspace flows. GPUI also opens the website links and reads
//! and saves the Linear API key.
//!
//! [`account_data`]: crate::gpui_shell::screen::account_data
//! [`Account`]: crate::gpui_shell::screen::account_data::Account

use crate::core::cloud_catalog::{Notification, SpendState};
use crate::gpui_shell::native::Toast;
use crate::gpui_shell::ui::radius;
use crate::gpui_shell::ui::zpx;
use std::sync::Arc;

use gpui_kit::base::TestSupportExt as _;
use serde::Deserialize;
use tauri_plugin_gpui::gpui::{
  self, div, prelude::*, AnyElement, Context, Focusable as _, FontWeight, SharedString, Window,
};

use super::{help, SettingsView, Tab};
use crate::gpui_shell::{
  form,
  native::{self, NativeEvent},
  popup::{Side, TooltipContent},
  screen::{
    account_data::{self, CloudAction},
    ScreenAction,
  },
  theme::{self, Palette},
  tooltips::{HasTooltips, Tooltips},
  ui::{self, Button, Size, TextSize, Tone, Variant},
};

/// The repository setting the Linear API key is stored under.
const LINEAR_API_KEY: &str = "linear_api_key";
/// Where Linear users create a personal API key.
const LINEAR_API_KEYS_URL: &str = "https://linear.app/settings/account/security";

/// The header an Integrations card draws.
struct IntegrationCard {
  /// "github", "linear": its selector and toggle id.
  id: &'static str,
  icon: &'static str,
  name: &'static str,
  /// One line on what connecting it does.
  purpose: &'static str,
  connected: bool,
  open: bool,
}

/// The account as the tabs draw it: Rust's [`Account`], with what the tabs
/// load and run themselves.
///
/// [`Account`]: crate::gpui_shell::screen::account_data::Account
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AccountRoute {
  /// The stored session is still being restored.
  pub loading: bool,
  /// A sign-in token from a `treq://auth/callback` link is being exchanged.
  pub signing_in: bool,
  pub sign_in_error: Option<String>,
  /// Set while there is a session.
  pub user: Option<User>,
  pub subscription: Option<Subscription>,
  /// The dev build's pasted-callback exchange.
  pub dev_callback: DevCallbackRoute,
  pub cloud: Option<CloudRoute>,
  /// The `github_repositories` the account can see, while signed in.
  pub github: Option<GithubRoute>,
  /// "Connect via OAuth" is starting the flow.
  pub linear_oauth_loading: bool,
}

/// The cloud card's line for a running switch to a new machine.
#[derive(Debug, Clone, PartialEq)]
pub struct MigrationView {
  pub title: String,
  /// The last error and what to do, while a step keeps failing.
  pub detail: Option<String>,
  /// A step is failing: shown as a warning with a support hint.
  pub stuck: bool,
}

fn migration_step(state: &str) -> &'static str {
  match state {
    "requested" | "preflight" => "checking for unpushed work",
    "awaiting_ack" => "waiting for you to confirm",
    "target_provisioning" => "creating the new machine",
    "target_bootstrapped" => "cloning repositories",
    "verifying" => "checking the new machine",
    "cutover" => "switching over",
    "source_retiring" => "stopping the old machine",
    "rolling_back" => "undoing the move",
    _ => "working",
  }
}

fn migration_view(m: &crate::core::control_plane::MigrationSummary) -> MigrationView {
  let error = m.error_message();
  MigrationView {
    title: format!("Moving to a new machine: {}", migration_step(&m.state)),
    stuck: error.is_some(),
    detail: error.map(|e| {
      format!(
        "Last error: {}. If this doesn't clear, contact support with migration {}.",
        e.trim_end_matches('.'),
        m.id
      )
    }),
  }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct User {
  pub name: String,
  pub email: Option<String>,
  pub avatar_url: Option<String>,
}

/// A row of `public.subscriptions`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Subscription {
  pub plan: String,
  pub status: String,
  pub current_period_end: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DevCallbackRoute {
  pub loading: bool,
  pub error: Option<String>,
}

/// `CloudWorkspaceCardProps`, with the instance trimmed to what it draws.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CloudRoute {
  pub instance: Option<CloudInstance>,
  pub endpoint: Option<CloudEndpoint>,
  pub provisioning_stage: Option<String>,
  pub provisioning_error: Option<String>,
  pub usage: Option<UsageReport>,
  pub usage_error: Option<String>,
  /// One of the card's actions is running.
  pub submitting: bool,
  /// The priced size tiers.
  pub tiers: Vec<crate::core::cloud_catalog::Tier>,
  pub selected_tier: Option<String>,
  /// This month's billed usage.
  pub billing: Option<crate::core::cloud_catalog::UsageTotals>,
  /// The instance's open switch to a new machine.
  pub migration: Option<crate::core::control_plane::MigrationSummary>,
  /// A resize waiting for the user to accept losing this work.
  pub resize_preflight: Option<(String, crate::core::control_plane::Preflight)>,
  /// The idle controller's last probe.
  pub activity: Option<crate::core::workspace_activity::WorkspaceActivity>,
  /// The monthly spend cap and this month's spend.
  pub spend: Option<crate::core::cloud_catalog::SpendStatus>,
  /// Why the last cap change was refused.
  pub spend_cap_error: Option<String>,
  /// Stored notices, until dismissed.
  pub notifications: Vec<crate::core::cloud_catalog::Notification>,
  /// Bring your own sandbox: the saved provider keys (never the keys).
  pub provider_keys: Vec<crate::core::cloud_catalog::ProviderKey>,
  pub provider_key_saving: bool,
  pub provider_key_error: Option<String>,
  /// Tier prices on the providers the user has a key for.
  pub byos_catalog: Vec<crate::core::cloud_catalog::ByosMachineType>,
  /// Team cloud: the user's organizations and their cloud settings.
  pub organizations: Vec<crate::core::cloud_catalog::CloudOrganization>,
  /// Where the next workspace bills: personal or an organization.
  pub profile: Option<crate::core::cloud_catalog::CloudProfile>,
  /// Each Team organization's saved keys (never the keys).
  pub org_keys: std::collections::BTreeMap<String, Vec<crate::core::cloud_catalog::ProviderKey>>,
  pub team_saving: bool,
  pub team_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudInstance {
  /// A `ManagedInstanceState`, as in `"ready"`.
  pub status: String,
  pub provider_resource_id: Option<String>,
  pub disk_quota_gb: f64,
  #[serde(default)]
  pub lapsed_at: Option<String>,
  #[serde(default)]
  pub tier_id: Option<String>,
  #[serde(default)]
  pub region: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct CloudEndpoint {
  pub username: String,
  pub hostname: String,
  pub port: u16,
}

/// `MachineUsageReport`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageReport {
  pub repository_count: u64,
  pub workspace_count: u64,
  pub disk_used_bytes: u64,
}

thread_local! {
  /// The repositories the Integrations tab last read, shown while it reads
  /// them again.
  static LAST_GITHUB: std::cell::RefCell<Option<Vec<GithubRepo>>> = const { std::cell::RefCell::new(None) };
}

#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GithubRoute {
  pub loading: bool,
  pub failed: bool,
  pub repositories: Vec<GithubRepo>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GithubRepo {
  pub id: i64,
  pub full_name: String,
  pub private: bool,
}

/// The Account tab, as drawn.
#[derive(Debug, Clone, PartialEq)]
pub enum AccountTab {
  Loading,
  SigningIn,
  SignedOut {
    error: Option<String>,
    /// Dev builds only.
    dev: Option<DevCallback>,
    /// "Sorry to see you go", after this window deleted the account.
    farewell: bool,
  },
  SignedIn(Box<SignedIn>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct DevCallback {
  pub url: String,
  pub loading: bool,
  pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SignedIn {
  pub name: String,
  pub email: Option<String>,
  pub pro: bool,
  /// The status label and its color.
  pub status: Option<(String, StatusTone)>,
  /// What the period end means, and the date.
  pub period: Option<(String, String)>,
  pub manage_label: String,
  pub cloud: Option<CloudCard>,
  /// The spend cap row, once the spend status is read.
  pub spend_cap: Option<SpendCapEditor>,
  /// "Provider keys", for Pro.
  pub provider_keys: Option<ProviderKeysSection>,
  /// "Cloud billing": personal or a Team organization. Only for members of
  /// a Team organization.
  pub cloud_profile: Option<CloudProfileSection>,
  /// The cloud settings of each Team organization the user owns.
  pub org_cloud: Vec<OrgCloudSection>,
  /// The delete-account confirmation, while it is open.
  pub delete_account: Option<DeleteAccountDialog>,
}

/// "Delete your Treq account?": the email typed to confirm, and the
/// server's reason when it refused.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DeleteAccountDialog {
  pub typed: String,
  /// The typed text is the account's email (any case, trimmed).
  pub can_confirm: bool,
  pub deleting: bool,
  pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusTone {
  Good,
  Warn,
  Muted,
}

/// `CloudWorkspaceCard`.
#[derive(Debug, Clone, PartialEq)]
pub struct CloudCard {
  /// The instance there is to manage.
  pub instance: Option<CloudInstanceView>,
  /// The create flow, when there is none.
  pub create: Option<CloudCreate>,
  /// The server's sentence when it refused for want of Pro.
  pub pro_required: Option<String>,
  /// The first line of any other error.
  pub error: Option<String>,
  /// The rest of it: the HTTP status and correlation id, for support.
  pub error_details: Option<String>,
  /// The error is an expired session: offer to sign in again.
  pub sign_in: bool,
  pub submitting: bool,
  pub confirming_delete: bool,
  /// The size picker, one option per tier.
  pub sizes: Vec<SizeOption>,
  /// "Resize" shows: a tier other than the instance's is picked.
  pub resize: bool,
  /// This month's billed total and hours.
  pub billing: Option<String>,
  /// What a resize would lose, while it waits for confirmation.
  pub resize_confirm: Option<Vec<String>>,
  /// A running switch to a new machine.
  pub migration: Option<MigrationView>,
  /// Where this month's spend stands against the cap.
  pub spend_state: Option<SpendState>,
  /// What the warning or capped state means.
  pub spend_note: Option<String>,
  /// The server's sentence when it refused ensure/wake/resize over the cap.
  pub spend_cap_reached: Option<String>,
  /// Stored notices, until dismissed.
  pub notifications: Vec<Notification>,
  /// The background processes of a running workspace.
  pub processes: Vec<ProcessRow>,
  /// "Stops in N min if idle", while it runs.
  pub idle_stop: Option<String>,
  /// The provider refused the saved BYOS key.
  pub credential_invalid: Option<String>,
}

/// The "Provider keys" section (bring your own sandbox).
#[derive(Debug, Clone, PartialEq)]
pub struct ProviderKeysSection {
  pub rows: Vec<ProviderKeyRow>,
  /// Providers without a key yet: kind and name.
  pub addable: Vec<(String, String)>,
  /// The provider whose key field is open.
  pub editing: Option<String>,
  pub draft: String,
  pub saving: bool,
  pub error: Option<String>,
}

/// Where the next cloud workspace bills.
#[derive(Debug, Clone, PartialEq)]
pub struct CloudProfileSection {
  pub options: Vec<ProfileOption>,
  pub saving: bool,
  pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProfileOption {
  /// The organization, or `None` for personal.
  pub org_id: Option<String>,
  pub label: String,
  /// Who pays and what it may run.
  pub note: String,
  pub selected: bool,
}

/// An owner's cloud settings for one Team organization.
#[derive(Debug, Clone, PartialEq)]
pub struct OrgCloudSection {
  pub org_id: String,
  pub name: String,
  pub allow_managed: bool,
  pub allow_byos: bool,
  /// The cap fields' text: the draft, or the saved cap.
  pub org_cap: String,
  pub member_cap: String,
  /// The organization's keys, and providers that have none yet.
  pub keys: Vec<ProviderKeyRow>,
  pub addable: Vec<(String, String)>,
  /// The provider whose key field is open, and its draft.
  pub editing: Option<String>,
  pub draft: String,
  pub saving: bool,
  pub error: Option<String>,
}

/// An owner's unsaved changes to an organization's cloud settings.
#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct OrgDraft {
  allow_managed: Option<bool>,
  allow_byos: Option<bool>,
  org_cap: Option<String>,
  member_cap: Option<String>,
  /// Why the last Save was not sent.
  invalid: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProviderKeyRow {
  pub kind: String,
  pub name: String,
  pub status: String,
  pub invalid: bool,
  /// "Create workspace on this account" shows: no workspace yet and the key
  /// is valid.
  pub can_create: bool,
}

/// One background process on the cloud workspace.
#[derive(Debug, Clone, PartialEq)]
pub struct ProcessRow {
  pub pid: u32,
  pub command: String,
  /// `Agent · 3.0% CPU · :3000`.
  pub detail: String,
  /// Kept alive by a pin, which `pin_id` removes.
  pub pinned: bool,
  pub pin_id: Option<String>,
  /// Managed services (sshd) cannot be killed or pinned.
  pub killable: bool,
}

/// The monthly spend cap row in Account settings.
#[derive(Debug, Clone, PartialEq)]
pub struct SpendCapEditor {
  /// "Monthly spend cap: $50.00".
  pub label: String,
  pub editing: bool,
  pub draft: String,
  /// The local validation error, or the server's refusal.
  pub error: Option<String>,
}

/// One size tier in the picker.
#[derive(Debug, Clone, PartialEq)]
pub struct SizeOption {
  pub id: String,
  pub label: String,
  /// `2 vCPU · 4 GiB RAM · 10 GB disk`.
  pub spec: String,
  /// The hourly price in the instance's region.
  pub price: Option<String>,
  pub selected: bool,
  /// The tier the instance runs as.
  pub current: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CloudInstanceView {
  pub label: String,
  pub endpoint: Option<String>,
  pub repairable: bool,
  pub usage: Option<CloudUsage>,
  pub open_repositories: bool,
  pub wake: bool,
  /// Why and when the lapse sweep will delete the workspace.
  pub deletion: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CloudUsage {
  Loading,
  Error(String),
  Report {
    summary: String,
    size: String,
    percent: u32,
  },
}

#[derive(Debug, Clone, PartialEq)]
pub struct CloudCreate {
  pub label: String,
  pub disabled: bool,
  pub spinning: bool,
  pub stage: Option<String>,
}

/// The Integrations tab, as drawn.
#[derive(Debug, Clone, PartialEq)]
pub struct IntegrationsTab {
  pub github: GithubSection,
  /// Signed in to Treq Cloud, which connects GitHub.
  pub github_connected: bool,
  /// While the Linear integration preview is on.
  pub linear: Option<LinearSection>,
  /// The cards shown expanded ("github", "linear").
  pub expanded: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum GithubSection {
  Loading,
  SignedOut,
  SignedIn {
    scope: String,
    description: String,
    /// Each repository's full name and whether it is private.
    repositories: Vec<(String, bool)>,
  },
}

#[derive(Debug, Clone, PartialEq)]
pub struct LinearSection {
  /// False when no repository is open, which the key belongs to.
  pub repo: bool,
  /// An API key is set for the repository.
  pub connected: bool,
  pub description: String,
  pub editing: bool,
  pub draft: String,
  pub saving: bool,
  pub edit_label: String,
  /// The OAuth row shows (Pro).
  pub oauth: bool,
  /// Free plan: Linear needs Pro, so an upgrade prompt replaces the key.
  pub upgrade: bool,
  pub oauth_description: String,
  pub oauth_loading: bool,
  pub key_priority_note: bool,
}

/// The tabs' own state.
#[derive(Default)]
pub(super) struct State {
  /// The Integrations cards the user expanded.
  expanded: std::collections::BTreeSet<String>,
  /// Rust's account, as of the last change.
  rust: AccountRoute,
  /// The pasted token being exchanged, and why the last exchange failed.
  dev: DevCallbackRoute,
  /// The GitHub repositories, once asked for while signed in.
  github: Option<GithubRoute>,
  linear_oauth_loading: bool,
  /// The cloud workspace was read for this sign-in.
  cloud_read: bool,
  callback_url: String,
  /// "No token found in URL", from the last Go.
  callback_error: Option<String>,
  confirming_delete: bool,
  /// The spend cap field is open, with its draft and local error.
  spend_cap_editing: bool,
  spend_cap_focus: bool,
  spend_cap_draft: String,
  spend_cap_invalid: Option<String>,
  /// The open delete-account confirmation (`can_confirm` is derived).
  delete_account: Option<DeleteAccountDialog>,
  /// The provider whose key field is open, its draft, and focus on next draw.
  byos_key_editing: Option<String>,
  byos_key_draft: String,
  byos_key_focus: bool,
  /// Team cloud: unsaved organization settings, and the open organization
  /// key field (organization and provider), its draft and focus.
  team_drafts: std::collections::BTreeMap<String, OrgDraft>,
  team_key_editing: Option<(String, String)>,
  team_key_draft: String,
  team_key_focus: bool,
  /// The account was deleted here; the signed-out tab says goodbye.
  farewell: bool,
  /// The avatar URL, and its image once loaded.
  avatar: Option<(String, Option<Arc<gpui::Image>>)>,
  linear: Option<Linear>,
}

/// The Linear API key of one repository.
struct Linear {
  path: String,
  key: Option<String>,
  editing: bool,
  /// The key field takes focus on its next draw, once it opens.
  focus: bool,
  draft: String,
  saving: bool,
}

/// `isProSubscription`: statuses that still carry Pro.
fn is_pro(subscription: Option<&Subscription>) -> bool {
  subscription.is_some_and(|s| {
    s.plan == "pro" && ["trialing", "active", "past_due", "canceled"].contains(&s.status.as_str())
  })
}

/// A renewal payment failed and Stripe is retrying it.
fn past_due(subscription: Option<&Subscription>) -> bool {
  subscription.is_some_and(|s| s.status == "past_due")
}

/// `subscriptionStatusLabel`.
fn status_label(status: &str) -> String {
  match status {
    "trialing" => "Trial",
    "active" => "Active",
    "past_due" => "Payment failed",
    "canceled" => "Canceling",
    "inactive" => "Inactive",
    other => other,
  }
  .to_string()
}

/// `subscriptionPeriodLabel`: what `current_period_end` means.
fn period_label(status: &str) -> &'static str {
  match status {
    "trialing" => "Trial ends",
    "active" => "Renews on",
    "past_due" => "Billing period ends",
    "canceled" => "Pro until",
    _ => "Period ends",
  }
}

/// `toLocaleDateString()` in an en-US locale.
/// Days a lapsed cloud workspace is kept before the sweep deletes it
/// (supabase/migrations/057_lapse_grace_30_days.sql).
const LAPSE_GRACE_DAYS: i64 = 30;

/// The warning for a workspace whose owner's Pro ended at `lapsed_at`.
fn deletion_notice(lapsed_at: &str) -> Option<String> {
  let lapsed = chrono::DateTime::parse_from_rfc3339(lapsed_at).ok()?;
  let deletes = (lapsed + chrono::Duration::days(LAPSE_GRACE_DAYS)).to_rfc3339();
  Some(format!(
    "Pro ended, so this workspace will be deleted on {}. Resubscribe to keep it.",
    local_date(&deletes)
  ))
}

fn local_date(at: &str) -> String {
  chrono::DateTime::parse_from_rfc3339(at)
    .map(|at| {
      at.with_timezone(&chrono::Local)
        .format("%-m/%-d/%Y")
        .to_string()
    })
    .unwrap_or_else(|_| at.to_string())
}

/// Whether the signed-out Account tab offers the pasted-callback box: in
/// `cargo run` builds, where `treq://` links don't reach the app, but not in
/// the e2e build that QA screenshots as the shipped app.
const DEV_CALLBACK: bool = cfg!(all(debug_assertions, not(feature = "e2e")));

/// The token in a pasted `treq://auth/callback?token=…` link, or the
/// pasted text itself.
fn callback_token(pasted: &str) -> Result<String, String> {
  let pasted = pasted.trim();
  let token = if pasted.contains("token=") {
    let url = url::Url::parse(pasted).map_err(|_| "Invalid URL".to_string())?;
    url
      .query_pairs()
      .find(|(key, _)| key == "token")
      .map(|(_, value)| value.into_owned())
      .unwrap_or_default()
  } else {
    pasted.to_string()
  };
  if token.is_empty() {
    Err("No token found in URL".into())
  } else {
    Ok(token)
  }
}

/// `STAGE_LABELS`.
fn stage_label(status: &str) -> String {
  match status {
    "unprovisioned" => "Not provisioned",
    "provisioning" => "Creating cloud workspace…",
    "bootstrapping" => "Installing Treq, JJ, Git, and agents…",
    "installing_access" => "Configuring access…",
    "verifying" => "Verifying readiness…",
    "ready" => "Ready",
    "suspended" => "Suspended (idle)",
    "waking" => "Waking…",
    "reprovisioning" => "Reprovisioning…",
    "degraded" => "Degraded",
    "failed" => "Failed",
    "deleting" => "Deleting…",
    "deleted" => "Deleted",
    other => other,
  }
  .to_string()
}

/// How `remoteFunctionError` stores a refusal for want of Pro.
const PRO_REQUIRED_PREFIX: &str = "[pro_required] ";
/// How a spend cap refusal reads in `provisioning_error`.
const SPEND_CAP_PREFIX: &str = "[spend_cap_reached] ";

/// How an expired session reads in `provisioning_error` (`ApiError::friendly`).
const SESSION_EXPIRED: &str = "Your session expired.";

const GIB: f64 = 1024. * 1024. * 1024.;
const MIB: f64 = 1024. * 1024.;

fn format_bytes(bytes: f64) -> String {
  if bytes >= GIB {
    format!("{:.1} GB", bytes / GIB)
  } else {
    format!("{} MB", (bytes / MIB).round())
  }
}

fn plural(count: u64, singular: &str, plural: &str) -> String {
  format!("{count} {}", if count == 1 { singular } else { plural })
}

/// A JS number as `${n}` prints it.
fn js_number(n: f64) -> String {
  if n.fract() == 0. {
    format!("{}", n as i64)
  } else {
    n.to_string()
  }
}

fn first_line(text: &str) -> String {
  text.split('\n').next().unwrap_or_default().to_string()
}

fn origin_label(origin: &str) -> &'static str {
  match origin {
    "agent" => "Agent",
    "terminal" => "Terminal",
    "service" => "Service",
    _ => "Background",
  }
}

/// The background processes and the stop ETA of a running workspace.
fn process_rows(cloud: &CloudRoute, running: bool) -> (Vec<ProcessRow>, Option<String>) {
  let Some(activity) = cloud.activity.as_ref().filter(|_| running) else {
    return (Vec::new(), None);
  };
  let rows = activity
    .processes
    .iter()
    .map(|process| {
      let pin = activity.pin_for(process);
      let mut detail = format!(
        "{} · {:.1}% CPU",
        origin_label(&process.origin),
        process.cpu
      );
      for port in &process.ports {
        detail.push_str(&format!(" · :{port}"));
      }
      ProcessRow {
        pid: process.pid,
        command: process.command.clone(),
        detail,
        pinned: process.pinned || pin.is_some(),
        pin_id: pin.map(|p| p.id.clone()),
        killable: process.origin != "service",
      }
    })
    .collect();
  let idle_stop = activity
    .stops_at
    .as_deref()
    .and_then(|at| chrono::DateTime::parse_from_rfc3339(at).ok())
    .map(|at| {
      let minutes = (at.with_timezone(&chrono::Utc) - chrono::Utc::now()).num_minutes();
      if minutes < 1 {
        "Stops soon if idle".to_string()
      } else {
        format!("Stops in {minutes} min if idle")
      }
    });
  (rows, idle_stop)
}

pub(in crate::gpui_shell::screen) fn cloud_card(
  cloud: &CloudRoute,
  confirming_delete: bool,
) -> CloudCard {
  let record = cloud.instance.as_ref();
  // A deleted instance keeps its provider resource id, but there is nothing
  // left to manage.
  let instance = record.filter(|i| i.provider_resource_id.is_some() && i.status != "deleted");
  let creation_retry =
    record.is_some_and(|i| i.status == "failed" && i.provider_resource_id.is_none());
  let error = cloud.provisioning_error.as_deref();
  let pro_required = error.and_then(|e| e.strip_prefix(PRO_REQUIRED_PREFIX));
  let spend_cap_reached = error.and_then(|e| e.strip_prefix(SPEND_CAP_PREFIX));
  let spend = cloud.spend.as_ref();

  let creating = cloud.submitting && instance.is_none();
  let stage = cloud.provisioning_stage.clone().filter(|s| !s.is_empty());
  let current_tier = instance.and_then(|i| i.tier_id.clone());
  let region = instance.and_then(|i| i.region.as_deref());
  let selected = cloud
    .selected_tier
    .clone()
    .or_else(|| current_tier.clone())
    .or_else(|| cloud.tiers.first().map(|t| t.id.clone()));
  let credential_invalid = error
    .filter(|e| e.starts_with("[provider_credential_invalid]"))
    .map(|_| {
      "Your provider rejected the saved API key. Replace it under Provider keys.".to_string()
    });
  let other_error = error.filter(|_| {
    pro_required.is_none() && spend_cap_reached.is_none() && credential_invalid.is_none()
  });
  let sizes = cloud
    .tiers
    .iter()
    .map(|tier| SizeOption {
      id: tier.id.clone(),
      label: tier.label.clone(),
      spec: format!(
        "{} vCPU · {} GiB RAM · {} GB disk",
        tier.vcpu,
        js_number(tier.ram_gib),
        tier.disk_gb
      ),
      price: tier
        .hourly_usd(region)
        .map(crate::core::cloud_catalog::format_hourly),
      selected: selected.as_deref() == Some(tier.id.as_str()),
      current: current_tier.as_deref() == Some(tier.id.as_str()),
    })
    .collect();
  let (processes, idle_stop) = process_rows(cloud, instance.is_some_and(|i| i.status == "ready"));
  CloudCard {
    processes,
    idle_stop,
    sizes,
    migration: cloud.migration.as_ref().map(migration_view),
    resize: instance.is_some()
      && cloud.migration.is_none()
      && cloud.selected_tier.is_some()
      && cloud.selected_tier != current_tier,
    billing: match spend {
      Some(s) => Some(format!("This month: ${:.2} of ${:.2}", s.spent_usd, s.cap_usd)),
      None => cloud
        .billing
        .map(|b| format!("This month: ${:.2} for {:.1} hours", b.billed_usd, b.hours)),
    },
    spend_state: spend.map(|s| s.state),
    spend_note: spend.and_then(|s| match s.state {
      SpendState::Ok => None,
      SpendState::Warning => Some(format!(
        "You've used {}% of your monthly cap. Workspaces stop when you reach it.",
        (s.spent_usd / s.cap_usd * 100.).floor()
      )),
      SpendState::Capped => Some(
        "You reached your monthly cap. Your cloud workspaces are stopped until you raise the cap or the month ends (UTC).".into(),
      ),
    }),
    spend_cap_reached: spend_cap_reached.map(first_line),
    notifications: cloud.notifications.clone(),
    resize_confirm: cloud.resize_preflight.as_ref().map(|(_, preflight)| {
      let lines = preflight.summary_lines();
      if lines.is_empty() {
        vec!["Unpushed work on the cloud workspace".to_string()]
      } else {
        lines
      }
    }),
    instance: instance.map(|instance| {
      let ready = instance.status == "ready";
      let endpoint = cloud.endpoint.as_ref();
      CloudInstanceView {
        label: stage_label(&instance.status),
        endpoint: endpoint.map(|e| format!("{}@{}:{}", e.username, e.hostname, e.port)),
        repairable: ["degraded", "failed"].contains(&instance.status.as_str()),
        usage: ready.then(|| match (&cloud.usage_error, &cloud.usage) {
          (Some(err), _) => CloudUsage::Error(format!("Couldn't load usage: {}", first_line(err))),
          (None, None) => CloudUsage::Loading,
          (None, Some(usage)) => {
            let quota = instance.disk_quota_gb;
            let used = usage.disk_used_bytes as f64;
            CloudUsage::Report {
              summary: format!(
                "{} across {}",
                plural(usage.workspace_count, "workspace", "workspaces"),
                plural(usage.repository_count, "repository", "repositories")
              ),
              size: format!("{} of {} GB", format_bytes(used), js_number(quota)),
              percent: ((used / (quota * GIB)) * 100.).round().min(100.) as u32,
            }
          }
        }),
        open_repositories: ready && endpoint.is_some(),
        wake: ["suspended", "waking"].contains(&instance.status.as_str()),
        deletion: instance.lapsed_at.as_deref().and_then(deletion_notice),
      }
    }),
    create: instance.is_none().then(|| CloudCreate {
      label: if creating || stage.is_some() {
        "Creating…"
      } else if creation_retry {
        "Retry cloud workspace creation"
      } else {
        "Create cloud workspace"
      }
      .into(),
      disabled: cloud.submitting || stage.is_some(),
      spinning: creating || stage.is_some(),
      stage,
    }),
    pro_required: pro_required.map(first_line),
    error: other_error.map(first_line),
    error_details: other_error
      .and_then(|e| e.split_once('\n'))
      .map(|(_, rest)| rest.trim().to_string())
      .filter(|rest| !rest.is_empty()),
    sign_in: other_error.is_some_and(|e| e.starts_with(SESSION_EXPIRED)),
    credential_invalid,
    submitting: cloud.submitting,
    confirming_delete,
  }
}

/// What an organization's cloud lets members run, as a sentence.
fn org_cloud_note(org: &crate::core::cloud_catalog::CloudOrganization) -> String {
  let name = &org.name;
  match (org.allow_managed, org.allow_byos) {
    (true, true) => format!("Bills {name}. treq-managed sandboxes or {name}'s own keys."),
    (true, false) => format!("Bills {name}. treq-managed sandboxes."),
    (false, true) => format!("Bills {name}. {name}'s own keys."),
    (false, false) => format!("Cloud is off for {name}. An owner can turn it on."),
  }
}

/// The profile switcher (members of a Team organization) and the owner's
/// settings for each Team organization they own.
fn team_sections(
  cloud: Option<&CloudRoute>,
  state: &State,
  pro: bool,
) -> (Option<CloudProfileSection>, Vec<OrgCloudSection>) {
  let Some(cloud) = cloud else {
    return (None, Vec::new());
  };
  let teams: Vec<_> = cloud.organizations.iter().filter(|o| o.has_team).collect();
  if teams.is_empty() {
    return (None, Vec::new());
  }
  let current = cloud.profile.as_ref().and_then(|p| p.org_id.as_deref());
  let no_workspace = cloud
    .instance
    .as_ref()
    .filter(|i| i.provider_resource_id.is_some() && i.status != "deleted")
    .is_none();
  let profile = CloudProfileSection {
    options: std::iter::once(ProfileOption {
      org_id: None,
      label: "Personal".into(),
      note: if pro {
        "Bills you.".into()
      } else {
        "Needs Pro.".into()
      },
      selected: current.is_none(),
    })
    .chain(teams.iter().map(|o| ProfileOption {
      org_id: Some(o.org_id.clone()),
      label: o.name.clone(),
      note: org_cloud_note(o),
      selected: current == Some(o.org_id.as_str()),
    }))
    .collect(),
    saving: cloud.team_saving,
    error: cloud.team_error.clone(),
  };
  let owned = teams
    .iter()
    .filter(|o| o.is_owner())
    .map(|o| {
      let draft = state
        .team_drafts
        .get(&o.org_id)
        .cloned()
        .unwrap_or_default();
      let saved = cloud.org_keys.get(&o.org_id).cloned().unwrap_or_default();
      // "Create workspace on this account" runs on the selected profile, so
      // it shows on this organization's keys only while it is selected.
      let creates_here = no_workspace && o.allow_byos && current == Some(o.org_id.as_str());
      let keys: Vec<ProviderKeyRow> = saved
        .iter()
        .map(|key| {
          let name = crate::core::cloud_catalog::provider_name(&key.provider_kind);
          let invalid = key.invalid_at.is_some();
          ProviderKeyRow {
            kind: key.provider_kind.clone(),
            status: if invalid {
              format!("Rejected by {name}. Replace the key.")
            } else {
              "Key saved".into()
            },
            name,
            invalid,
            can_create: creates_here && !invalid && !cloud.submitting,
          }
        })
        .collect();
      OrgCloudSection {
        org_id: o.org_id.clone(),
        name: o.name.clone(),
        allow_managed: draft.allow_managed.unwrap_or(o.allow_managed),
        allow_byos: draft.allow_byos.unwrap_or(o.allow_byos),
        org_cap: draft
          .org_cap
          .clone()
          .unwrap_or_else(|| js_number(o.org_cap_usd)),
        member_cap: draft
          .member_cap
          .clone()
          .unwrap_or_else(|| js_number(o.member_cap_usd)),
        addable: crate::core::cloud_catalog::BYOS_PROVIDERS
          .iter()
          .filter(|(kind, _)| !keys.iter().any(|r| r.kind == *kind))
          .map(|(kind, name)| (kind.to_string(), name.to_string()))
          .collect(),
        keys,
        editing: state
          .team_key_editing
          .as_ref()
          .filter(|(org, _)| *org == o.org_id)
          .map(|(_, kind)| kind.clone()),
        draft: state.team_key_draft.clone(),
        saving: cloud.team_saving,
        error: draft.invalid.or_else(|| cloud.team_error.clone()),
      }
    })
    .collect();
  (Some(profile), owned)
}

impl SettingsView {
  /// Rust's account with what the tabs loaded.
  fn account_route(&self) -> Option<AccountRoute> {
    let state = &self.account;
    Some(AccountRoute {
      dev_callback: state.dev.clone(),
      github: state.github.clone(),
      linear_oauth_loading: state.linear_oauth_loading,
      ..state.rust.clone()
    })
  }

  fn signed_in_pro(&self) -> bool {
    is_pro(self.account.rust.subscription.as_ref())
  }

  pub(super) fn account_tab(&self) -> AccountTab {
    let Some(account) = self.account_route().filter(|a| !a.loading) else {
      return AccountTab::Loading;
    };
    if account.signing_in {
      return AccountTab::SigningIn;
    }
    let state = &self.account;
    let Some(user) = &account.user else {
      return AccountTab::SignedOut {
        error: account
          .sign_in_error
          .as_ref()
          .map(|err| format!("Sign-in failed: {err}")),
        dev: DEV_CALLBACK.then(|| DevCallback {
          url: state.callback_url.clone(),
          loading: account.dev_callback.loading,
          error: state
            .callback_error
            .clone()
            .or_else(|| account.dev_callback.error.clone()),
        }),
        farewell: state.farewell,
      };
    };
    let subscription = account.subscription.as_ref();
    let pro = is_pro(subscription);
    // Bring your own sandbox shows only for Pro.
    let byos = pro;
    let mut cloud_route = account.cloud.clone();
    if !byos {
      if let Some(cloud) = cloud_route.as_mut() {
        cloud.provider_keys.clear();
        cloud.byos_catalog.clear();
        cloud.provider_key_error = None;
      }
    }
    let provider_keys = cloud_route.as_ref().filter(|_| byos).map(|cloud| {
      let no_workspace = cloud
        .instance
        .as_ref()
        .filter(|i| i.provider_resource_id.is_some() && i.status != "deleted")
        .is_none();
      let rows: Vec<ProviderKeyRow> = cloud
        .provider_keys
        .iter()
        .map(|key| {
          let name = crate::core::cloud_catalog::provider_name(&key.provider_kind);
          let invalid = key.invalid_at.is_some();
          ProviderKeyRow {
            kind: key.provider_kind.clone(),
            status: if invalid {
              format!("Rejected by {name}. Replace the key.")
            } else {
              "Key saved".into()
            },
            name,
            invalid,
            can_create: no_workspace && !invalid && !cloud.submitting,
          }
        })
        .collect();
      ProviderKeysSection {
        addable: crate::core::cloud_catalog::BYOS_PROVIDERS
          .iter()
          .filter(|(kind, _)| !rows.iter().any(|r| r.kind == *kind))
          .map(|(kind, name)| (kind.to_string(), name.to_string()))
          .collect(),
        rows,
        editing: state.byos_key_editing.clone(),
        draft: state.byos_key_draft.clone(),
        saving: cloud.provider_key_saving,
        error: cloud.provider_key_error.clone(),
      }
    });
    let (cloud_profile, org_cloud) = team_sections(cloud_route.as_ref(), state, pro);
    AccountTab::SignedIn(Box::new(SignedIn {
      provider_keys,
      cloud_profile,
      org_cloud,
      name: user.name.clone(),
      email: user.email.clone(),
      pro,
      status: subscription.filter(|s| !s.status.is_empty()).map(|s| {
        let tone = match s.status.as_str() {
          "active" | "trialing" => StatusTone::Good,
          "canceled" | "past_due" => StatusTone::Warn,
          _ => StatusTone::Muted,
        };
        (status_label(&s.status), tone)
      }),
      period: subscription.and_then(|s| {
        let end = s.current_period_end.as_ref()?;
        Some((period_label(&s.status).to_string(), local_date(end)))
      }),
      manage_label: if past_due(subscription) {
        "Update payment method"
      } else if pro {
        "Manage subscription"
      } else {
        "Upgrade to Pro"
      }
      .into(),
      cloud: cloud_route
        .as_ref()
        .map(|cloud| cloud_card(cloud, state.confirming_delete)),
      spend_cap: account
        .cloud
        .as_ref()
        .and_then(|cloud| cloud.spend.as_ref().map(|s| (cloud, s)))
        .map(|(cloud, s)| SpendCapEditor {
          label: format!("Monthly spend cap: ${:.2}", s.cap_usd),
          editing: state.spend_cap_editing,
          draft: state.spend_cap_draft.clone(),
          error: state
            .spend_cap_invalid
            .clone()
            .or_else(|| cloud.spend_cap_error.clone()),
        }),
      delete_account: state.delete_account.as_ref().map(|d| DeleteAccountDialog {
        can_confirm: !d.deleting && email_matches(user.email.as_deref(), &d.typed),
        ..d.clone()
      }),
    }))
  }

  pub(super) fn integrations_tab(&self) -> IntegrationsTab {
    let account = self.account_route();
    let github = match &account {
      None => GithubSection::Loading,
      Some(account) if account.loading => GithubSection::Loading,
      Some(AccountRoute { user: None, .. }) => GithubSection::SignedOut,
      Some(account) => {
        let pro = is_pro(account.subscription.as_ref());
        let github = account.github.clone().unwrap_or(GithubRoute {
          loading: true,
          ..GithubRoute::default()
        });
        let visible: Vec<_> = github
          .repositories
          .iter()
          .filter(|repo| pro || !repo.private)
          .map(|repo| (repo.full_name.clone(), repo.private))
          .collect();
        let description = if github.loading {
          "Loading GitHub repositories…".to_string()
        } else if github.failed {
          "Could not load GitHub repositories. Try again later.".to_string()
        } else if visible.is_empty() {
          "No enabled GitHub repositories.".to_string()
        } else {
          format!("{} enabled.", visible.len())
        };
        GithubSection::SignedIn {
          scope: if pro {
            "All repositories"
          } else {
            "Public repositories"
          }
          .into(),
          description,
          repositories: if github.loading || github.failed {
            Vec::new()
          } else {
            visible
          },
        }
      }
    };
    let pro = self.signed_in_pro();
    let linear = self
      .preview_on(crate::core::feature_preview::PreviewFeature::LinearIntegration)
      .then(|| {
        let linear = self
          .account
          .linear
          .as_ref()
          .filter(|l| Some(&l.path) == self.route.repo_path.as_ref());
        let key = linear.and_then(|l| l.key.clone());
        LinearSection {
          repo: self.repo.is_some(),
          connected: key.is_some(),
          description: if key.is_some() {
            "Direct API access configured"
          } else {
            "Paste a Linear personal API key"
          }
          .into(),
          editing: linear.is_some_and(|l| l.editing),
          draft: linear.map(|l| l.draft.clone()).unwrap_or_default(),
          saving: linear.is_some_and(|l| l.saving),
          edit_label: format!("{} API key", if key.is_some() { "Update" } else { "Add" }),
          oauth: pro && native::feature_flag("pro"),
          upgrade: !pro && !crate::linear::e2e_pro(),
          oauth_description: if key.is_some() {
            "API key takes priority, but you can also connect via OAuth"
          } else {
            "Pro plan: connect via OAuth"
          }
          .into(),
          oauth_loading: account.as_ref().is_some_and(|a| a.linear_oauth_loading),
          key_priority_note: key.is_some() && pro,
        }
      });
    IntegrationsTab {
      github_connected: matches!(github, GithubSection::SignedIn { .. }),
      github,
      linear,
      expanded: self.account.expanded.iter().cloned().collect(),
    }
  }

  /// Opens or closes an Integrations card.
  pub(super) fn toggle_integration(&mut self, id: &str) {
    if !self.account.expanded.remove(id) {
      self.account.expanded.insert(id.to_string());
    }
  }

  /// Reads the Linear API key each time the Integrations tab shows, as
  /// `LinearIntegrationSettings` does when it mounts.
  pub(super) fn load_linear(&mut self, cx: &mut Context<Self>) {
    let Some(path) = self.route.repo_path.clone().filter(|p| !p.is_empty()) else {
      self.account.linear = None;
      return;
    };
    self.account.linear = Some(Linear {
      path: path.clone(),
      key: None,
      editing: false,
      focus: false,
      draft: String::new(),
      saving: false,
    });
    self.spawn(
      cx,
      {
        let path = path.clone();
        move |host| host.repo_settings(&path, &[LINEAR_API_KEY.to_string()])
      },
      move |view, values, _| {
        let Some(linear) = view.account.linear.as_mut().filter(|l| l.path == path) else {
          return;
        };
        linear.key = values
          .ok()
          .and_then(|mut values| values.remove(LINEAR_API_KEY).flatten())
          .filter(|key| !key.is_empty());
      },
    );
  }

  fn save_linear_key(&mut self, cx: &mut Context<Self>) {
    let Some(linear) = self
      .account
      .linear
      .as_mut()
      .filter(|l| !l.saving && !l.draft.is_empty())
    else {
      return;
    };
    linear.saving = true;
    let (path, key) = (linear.path.clone(), linear.draft.clone());
    self.spawn(
      cx,
      {
        let (path, key) = (path.clone(), key.clone());
        move |host| {
          let result = host.set_repo_settings(&path, &[(LINEAR_API_KEY.to_string(), key)]);
          match &result {
            Ok(()) => {
              host.notify(NativeEvent::RepoSettingsChanged { repo_path: path });
              host.notify(NativeEvent::from(Toast::success("API key saved")));
            }
            Err(err) => host.notify(NativeEvent::from(
              Toast::error("Error saving API key").description(err.clone()),
            )),
          }
          result
        }
      },
      move |view, result, _| {
        let Some(linear) = view.account.linear.as_mut().filter(|l| l.path == path) else {
          return;
        };
        linear.saving = false;
        if result.is_ok() {
          linear.editing = false;
          linear.key = Some(key);
        }
      },
    );
  }

  /// Loads the avatar React's user has, once per URL.
  pub(super) fn sync_avatar(&mut self, cx: &mut Context<Self>) {
    let url = self
      .account
      .rust
      .user
      .as_ref()
      .and_then(|u| u.avatar_url.clone())
      .filter(|url| !url.is_empty());
    if self.account.avatar.as_ref().map(|(u, _)| u) == url.as_ref() {
      return;
    }
    self.account.avatar = url.clone().map(|url| (url, None));
    let Some(url) = url else {
      return;
    };
    self.spawn(
      cx,
      {
        let url = url.clone();
        move |host| host.fetch_image(&url)
      },
      move |view, bytes, _| {
        let image = bytes.ok().and_then(treq_ui::image::decode);
        if let Some((current, slot)) = &mut view.account.avatar {
          if *current == url {
            *slot = image;
          }
        }
      },
    );
  }

  /// Takes Rust's account, then loads what the shown tab needs of it.
  /// Signing in clears the pasted callback, as a successful Go does.
  pub(super) fn sync_account(&mut self, cx: &mut Context<Self>) {
    self.account.rust = account_data::account(cx);
    let signed_in = self.account.rust.user.is_some();
    if signed_in {
      self.account.callback_url.clear();
      self.account.callback_error = None;
      self.account.farewell = false;
    } else {
      self.account.github = None;
      LAST_GITHUB.with(|last| *last.borrow_mut() = None);
      self.account.cloud_read = false;
    }
    self.sync_avatar(cx);
    if !signed_in || self.account.rust.loading {
      return;
    }
    // `AccountSettings` reads the cloud workspace once per sign-in.
    if self.route.tab == Tab::Account && !self.account.cloud_read {
      self.account.cloud_read = true;
      self.host.cloud_action(CloudAction::Refresh);
    }
    if self.route.tab == Tab::Integrations && self.account.github.is_none() {
      self.load_github(cx);
    }
  }

  /// Reads the GitHub repositories. Those read last time show meanwhile,
  /// so reopening the tab has no spinner.
  fn load_github(&mut self, cx: &mut Context<Self>) {
    let last = LAST_GITHUB.with(|last| last.borrow().clone());
    self.account.github = Some(match last.clone() {
      Some(repositories) => GithubRoute {
        loading: false,
        failed: false,
        repositories,
      },
      None => GithubRoute {
        loading: true,
        ..GithubRoute::default()
      },
    });
    self.spawn(
      cx,
      |host| host.github_repositories(),
      move |view, result, _| {
        view.account.github = Some(match result {
          Ok(repositories) => {
            let repositories: Vec<GithubRepo> = repositories
              .into_iter()
              .map(|r| GithubRepo {
                id: r.id,
                full_name: r.full_name,
                private: r.private,
              })
              .collect();
            LAST_GITHUB.with(|last| *last.borrow_mut() = Some(repositories.clone()));
            GithubRoute {
              loading: false,
              failed: false,
              repositories,
            }
          }
          Err(error) if last.is_some() => {
            log::warn!("reading the GitHub repositories failed: {error}");
            return;
          }
          Err(error) => {
            log::warn!("reading the GitHub repositories failed: {error}");
            GithubRoute {
              loading: false,
              failed: true,
              repositories: Vec::new(),
            }
          }
        });
      },
    );
  }

  /// The dev build's pasted-callback exchange.
  fn exchange_pasted(&mut self, token: String, cx: &mut Context<Self>) {
    self.account.dev = DevCallbackRoute {
      loading: true,
      error: None,
    };
    self.spawn(
      cx,
      move |host| host.exchange_token(&token),
      |view, result, _| {
        view.account.dev = DevCallbackRoute {
          loading: false,
          error: result.err(),
        };
      },
    );
  }

  /// Deletes the account once the typed email matches. On success the
  /// account signs out and the tab falls back to "Sign in"; a refusal stays
  /// in the dialog with the server's reason.
  fn delete_account(&mut self, cx: &mut Context<Self>) {
    let email = self
      .account_route()
      .and_then(|a| a.user)
      .and_then(|u| u.email);
    let Some(dialog) = &mut self.account.delete_account else {
      return;
    };
    if dialog.deleting || !email_matches(email.as_deref(), &dialog.typed) {
      return;
    }
    dialog.deleting = true;
    dialog.error = None;
    self.spawn(
      cx,
      |host| host.delete_account(),
      |view, result, _| match result {
        Ok(()) => {
          view.account.delete_account = None;
          view.account.farewell = true;
          view.host.notify(NativeEvent::from(
            Toast::success("Account deleted")
              .description("Your Treq account and its data have been deleted."),
          ));
        }
        Err(error) => {
          if let Some(dialog) = &mut view.account.delete_account {
            dialog.deleting = false;
            dialog.error = Some(error.message);
          }
        }
      },
    );
  }

  /// `startLinearOAuth`: opens Linear's OAuth page, or toasts why not.
  fn start_linear_oauth(&mut self, cx: &mut Context<Self>) {
    if self.account.linear_oauth_loading {
      return;
    }
    self.account.linear_oauth_loading = true;
    self.spawn(
      cx,
      |host| host.linear_oauth_url(),
      |view, result, _| {
        view.account.linear_oauth_loading = false;
        let host = view.host.clone();
        match result {
          Ok(url) => host.open_url(&url),
          // The server decides entitlement; the Pro gate is display only.
          Err(error) if error.is_pro_required() => {
            let opener = host.clone();
            let id = native::register_toast_action(move || {
              opener.open_url(&format!("{}/dashboard", native::web_url()))
            });
            host.notify(NativeEvent::from(
              Toast::warning("Linear OAuth needs Pro")
                .description("Add a personal Linear API key above to use Linear on the free plan.")
                .action(native::ToastAction {
                  label: "Upgrade to Pro".into(),
                  id,
                }),
            ));
          }
          Err(error) => host.notify(NativeEvent::from(
            Toast::error("Error starting OAuth flow").description(error.message),
          )),
        }
      },
    );
  }

  /// The account and integrations buttons GPUI handles; false for the rest.
  pub(super) fn press_account(&mut self, id: &str, cx: &mut Context<Self>) -> bool {
    let web = native::web_url();
    if let Some(action) = account_data::cloud_press(id) {
      self.host.cloud_action(action);
      return true;
    }
    match id {
      "account.signIn" => self.host.open_url(&format!("{web}/sign-in?source=desktop")),
      // A failed payment goes straight to the subscription tab, where the
      // billing portal updates the card.
      "account.manage" if past_due(self.account_route().and_then(|a| a.subscription).as_ref()) => {
        self
          .host
          .open_url(&format!("{web}/dashboard?tab=subscription"))
      }
      "account.manage" | "cloud.upgrade" => self.host.open_url(&format!("{web}/dashboard")),
      "github.manage" => self
        .host
        .open_url(&format!("{web}/dashboard?tab=integrations")),
      "integration.toggle.github" => self.toggle_integration("github"),
      "integration.toggle.linear" => self.toggle_integration("linear"),
      "linear.createKey" => self.host.open_url(LINEAR_API_KEYS_URL),
      "account.devCallback.go" => match callback_token(&self.account.callback_url) {
        Ok(token) => {
          self.account.callback_error = None;
          self.exchange_pasted(token, cx);
        }
        Err(err) => self.account.callback_error = Some(err),
      },
      "account.signOut" => self.host.sign_out(),
      "account.delete.ask" => self.account.delete_account = Some(DeleteAccountDialog::default()),
      "account.delete.cancel" => {
        if !self
          .account
          .delete_account
          .as_ref()
          .is_some_and(|d| d.deleting)
        {
          self.account.delete_account = None;
        }
      }
      "account.delete.confirm" => self.delete_account(cx),
      "account.farewell.dismiss" => self.account.farewell = false,
      "linear.oauth" => self.start_linear_oauth(cx),
      "cloud.provision" => {
        let host = self.host.clone();
        account_data::provision_once(cx, || host.cloud_action(CloudAction::Provision));
        self.account.rust = account_data::account(cx);
      }
      "cloud.wake" => self.host.cloud_action(CloudAction::Wake),
      "cloud.repair" => self.host.cloud_action(CloudAction::Repair),
      "cloud.openRepositories" => {
        self.host.cloud_action(CloudAction::OpenRepositories);
        (self.react)(
          ScreenAction::Press {
            id: "settings.close".into(),
          },
          cx,
        );
      }
      "cloud.delete.ask" => self.account.confirming_delete = true,
      "cloud.delete.cancel" => self.account.confirming_delete = false,
      "cloud.delete.confirm" => {
        self.account.confirming_delete = false;
        self.host.cloud_action(CloudAction::Delete);
      }
      "linear.apiKey.edit" => {
        if let Some(linear) = &mut self.account.linear {
          linear.editing = true;
          linear.focus = true;
          linear.draft = linear.key.clone().unwrap_or_default();
        }
      }
      "linear.apiKey.cancel" => {
        if let Some(linear) = &mut self.account.linear {
          linear.editing = false;
          linear.draft.clear();
        }
      }
      "linear.apiKey.save" => self.save_linear_key(cx),
      "byos.key.cancel" => {
        self.account.byos_key_editing = None;
        self.account.byos_key_draft.clear();
      }
      "byos.key.save" => {
        let draft = std::mem::take(&mut self.account.byos_key_draft);
        if let (Some(kind), false) = (
          self.account.byos_key_editing.take(),
          draft.trim().is_empty(),
        ) {
          self.host.cloud_action(CloudAction::SaveProviderKey {
            kind,
            key: crate::commands::cloud::ApiKey(draft.trim().to_string()),
          });
        }
      }
      _ if id.starts_with("byos.key.add.") => {
        self.account.byos_key_editing = Some(
          id.strip_prefix("byos.key.add.")
            .unwrap_or_default()
            .to_string(),
        );
        self.account.byos_key_draft.clear();
        self.account.byos_key_focus = true;
      }
      _ if id.starts_with("byos.create.") => {
        self.host.cloud_action(CloudAction::ProvisionOn {
          kind: id
            .strip_prefix("byos.create.")
            .unwrap_or_default()
            .to_string(),
        });
      }
      _ if id.starts_with("byos.key.remove.") => {
        self.host.cloud_action(CloudAction::RemoveProviderKey {
          kind: id
            .strip_prefix("byos.key.remove.")
            .unwrap_or_default()
            .to_string(),
        });
      }
      "team.profile.personal" => self
        .host
        .cloud_action(CloudAction::SetCloudProfile { org_id: None }),
      _ if id.starts_with("team.profile.") => {
        let org_id = id
          .strip_prefix("team.profile.")
          .unwrap_or_default()
          .to_string();
        self.host.cloud_action(CloudAction::SetCloudProfile {
          org_id: Some(org_id),
        });
      }
      _ if id.starts_with("team.save.") => {
        let org_id = id
          .strip_prefix("team.save.")
          .unwrap_or_default()
          .to_string();
        self.save_org_cloud(&org_id);
      }
      "team.key.cancel" => {
        self.account.team_key_editing = None;
        self.account.team_key_draft.clear();
      }
      "team.key.save" => {
        let draft = std::mem::take(&mut self.account.team_key_draft);
        if let (Some((org_id, kind)), false) = (
          self.account.team_key_editing.take(),
          draft.trim().is_empty(),
        ) {
          self.host.cloud_action(CloudAction::SaveOrgProviderKey {
            org_id,
            kind,
            key: crate::commands::cloud::ApiKey(draft.trim().to_string()),
          });
        }
      }
      _ if id.starts_with("team.key.add.") => {
        if let Some((org_id, kind)) = id
          .strip_prefix("team.key.add.")
          .and_then(|r| r.split_once('.'))
        {
          self.account.team_key_editing = Some((org_id.to_string(), kind.to_string()));
          self.account.team_key_draft.clear();
          self.account.team_key_focus = true;
        }
      }
      _ if id.starts_with("team.key.remove.") => {
        if let Some((org_id, kind)) = id
          .strip_prefix("team.key.remove.")
          .and_then(|r| r.split_once('.'))
        {
          self.host.cloud_action(CloudAction::RemoveOrgProviderKey {
            org_id: org_id.to_string(),
            kind: kind.to_string(),
          });
        }
      }
      "cloud.spendCap.edit" => {
        let cap = self
          .account_route()
          .and_then(|a| a.cloud)
          .and_then(|c| c.spend)
          .map(|s| js_number(s.cap_usd))
          .unwrap_or_default();
        self.account.spend_cap_editing = true;
        self.account.spend_cap_focus = true;
        self.account.spend_cap_draft = cap;
        self.account.spend_cap_invalid = None;
      }
      "cloud.spendCap.cancel" => {
        self.account.spend_cap_editing = false;
        self.account.spend_cap_invalid = None;
      }
      "cloud.spendCap.save" => match parse_spend_cap(&self.account.spend_cap_draft) {
        Ok(cap_usd) => {
          self.account.spend_cap_editing = false;
          self.account.spend_cap_invalid = None;
          self.host.cloud_action(CloudAction::SetSpendCap { cap_usd });
        }
        Err(error) => self.account.spend_cap_invalid = Some(error),
      },
      _ => return false,
    }
    true
  }

  /// The account and integrations fields; false for the rest.
  pub(super) fn set_account_field(&mut self, field: &str, value: &serde_json::Value) -> bool {
    let text = value.as_str().unwrap_or_default().to_string();
    match field {
      "account.callbackUrl" => self.account.callback_url = text,
      "account.delete.email" => {
        if let Some(dialog) = &mut self.account.delete_account {
          dialog.typed = text;
        }
      }
      "linear.apiKey" => {
        if let Some(linear) = &mut self.account.linear {
          linear.draft = text;
        }
      }
      "cloud.spendCap" => self.account.spend_cap_draft = text,
      "byos.key" => self.account.byos_key_draft = text,
      "team.key" => self.account.team_key_draft = text,
      _ if field.starts_with("team.cap.org.") => {
        let org_id = field.strip_prefix("team.cap.org.").unwrap_or_default();
        self
          .account
          .team_drafts
          .entry(org_id.to_string())
          .or_default()
          .org_cap = Some(text);
      }
      _ if field.starts_with("team.cap.member.") => {
        let org_id = field.strip_prefix("team.cap.member.").unwrap_or_default();
        self
          .account
          .team_drafts
          .entry(org_id.to_string())
          .or_default()
          .member_cap = Some(text);
      }
      _ if field.starts_with("team.managed.") => {
        let org_id = field.strip_prefix("team.managed.").unwrap_or_default();
        self
          .account
          .team_drafts
          .entry(org_id.to_string())
          .or_default()
          .allow_managed = value.as_bool();
      }
      _ if field.starts_with("team.byos.") => {
        let org_id = field.strip_prefix("team.byos.").unwrap_or_default();
        self
          .account
          .team_drafts
          .entry(org_id.to_string())
          .or_default()
          .allow_byos = value.as_bool();
      }
      _ => return false,
    }
    true
  }
}

impl SettingsView {
  /// Sends an owner's changes to an organization's cloud, or says why a cap
  /// is not accepted.
  fn save_org_cloud(&mut self, org_id: &str) {
    let Some(org) = self
      .account_route()
      .and_then(|a| a.cloud)
      .and_then(|c| c.organizations.into_iter().find(|o| o.org_id == org_id))
    else {
      return;
    };
    let draft = self
      .account
      .team_drafts
      .entry(org_id.to_string())
      .or_default();
    let caps = parse_spend_cap(
      draft
        .org_cap
        .as_deref()
        .unwrap_or(&js_number(org.org_cap_usd)),
    )
    .and_then(|org_cap| {
      parse_spend_cap(
        draft
          .member_cap
          .as_deref()
          .unwrap_or(&js_number(org.member_cap_usd)),
      )
      .map(|m| (org_cap, m))
    });
    match caps {
      Err(error) => draft.invalid = Some(error),
      Ok((org_cap_usd, member_cap_usd)) => {
        let settings = CloudAction::SaveOrgCloudSettings {
          org_id: org_id.to_string(),
          allow_managed: draft.allow_managed.unwrap_or(org.allow_managed),
          allow_byos: draft.allow_byos.unwrap_or(org.allow_byos),
          org_cap_usd,
          member_cap_usd,
        };
        self.account.team_drafts.remove(org_id);
        self.host.cloud_action(settings);
      }
    }
  }
}

/// A typed cap in dollars ("80", "$80.50"). The server checks the maximum.
fn parse_spend_cap(typed: &str) -> Result<f64, String> {
  let typed = typed.trim().trim_start_matches('$').replace(',', "");
  match typed.parse::<f64>() {
    Ok(cap) if cap.is_finite() && cap >= 5. => Ok(cap),
    _ => Err("Enter an amount of at least $5.".into()),
  }
}

/// The typed confirmation names the account's email.
fn email_matches(email: Option<&str>, typed: &str) -> bool {
  email.is_some_and(|email| !email.is_empty() && typed.trim().eq_ignore_ascii_case(email))
}

/// `py-16` centered on the tab, with `gap`.
fn centered(gap: f32) -> gpui::Div {
  div()
    .flex()
    .flex_col()
    .items_center()
    .justify_center()
    .py_16()
    .gap(zpx(gap))
}

/// `<h3 className="text-lg font-semibold">`.
fn heading(text: &'static str) -> gpui::Div {
  ui::text(div(), TextSize::Lg)
    .font_weight(FontWeight::SEMIBOLD)
    .child(text)
}

/// "Sign in with browser", which opens the website's sign-in page.
fn sign_in_button(id: &'static str, view: &SettingsView, p: &Palette) -> Button {
  Button::new(id, p)
    .child(ui::icon(
      theme::ICON_EXTERNAL_LINK,
      16.,
      p.primary_foreground,
    ))
    .label("Sign in with browser")
    .on_click(view.press("account.signIn"))
}

/// `SettingRow`: a title and explanation on the left, a control on the
/// right.
fn setting_row(
  title: &str,
  description: &str,
  control: impl IntoElement,
  p: &Palette,
) -> gpui::Div {
  div()
    .flex()
    .items_start()
    .justify_between()
    .gap_4()
    .py_3()
    .child(
      ui::text(div(), TextSize::Sm)
        .min_w_0()
        .child(
          div()
            .font_weight(FontWeight::MEDIUM)
            .child(title.to_string()),
        )
        .child(
          div()
            .text_color(p.muted_foreground)
            .child(description.to_string()),
        ),
    )
    .child(
      div()
        .flex()
        .flex_none()
        .items_center()
        .gap_2()
        .child(control),
    )
}

/// Error text.
fn red(p: &Palette) -> gpui::Hsla {
  Tone::Danger.text(p)
}

/// A click handler that presses the button `id` of a view.
pub(in crate::gpui_shell::screen) type ClickFn =
  Box<dyn Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App)>;

/// Names the button `id` of a view's, for the view that draws a card.
pub(in crate::gpui_shell::screen) type PressFn<'a> = &'a dyn Fn(&str) -> ClickFn;

/// `CloudWorkspaceCard`. Its buttons press `cloud.repair`, `cloud.delete.ask`,
/// `cloud.openRepositories`, `cloud.wake`, `cloud.provision` and
/// `cloud.upgrade`, as `press` names them for the view that draws it.
pub(in crate::gpui_shell::screen) fn cloud_card_element<V: HasTooltips>(
  c: &CloudCard,
  p: &Palette,
  tips: &Tooltips,
  press: PressFn,
  cx: &mut Context<V>,
) -> impl IntoElement {
  let red = red(p);
  let actions = c.instance.as_ref().map(|instance| {
    let repair = instance.repairable.then(|| {
      let button = Button::new("cloud-repair", p)
        .variant(Variant::Outline)
        .size(Size::Sm)
        .h(zpx(28.))
        .gap_1p5()
        .disabled(c.submitting)
        .child(ui::icon(theme::ICON_WRENCH, 14., p.foreground))
        .label("Repair")
        .on_click(press("cloud.repair"));
      tips.wrap(
        "cloud-repair",
        Side::Bottom,
        Some(TooltipContent::Text(
          "Re-run Treq setup on this cloud workspace. Files and repositories are kept.".into(),
        )),
        button,
        cx,
      )
    });
    let delete = Button::new("cloud-delete", p)
      .aria_label("Delete this cloud workspace")
      .variant(Variant::Ghost)
      .size(Size::Icon)
      .w(zpx(28.))
      .h(zpx(28.))
      .disabled(c.submitting)
      .child(ui::icon(theme::ICON_TRASH, 16., p.muted_foreground))
      .on_click(press("cloud.delete.ask"));
    let delete = tips.wrap(
      "cloud-delete",
      Side::Bottom,
      Some(TooltipContent::Text(
        "Delete this cloud workspace and all data on it".into(),
      )),
      delete,
      cx,
    );
    div()
      .ml_auto()
      .flex()
      .items_center()
      .gap_1()
      .children(repair)
      .child(delete)
  });

  let sizes = (!c.sizes.is_empty()).then(|| {
    div()
      .debug_selector(|| "cloud-size-picker".into())
      .flex()
      .flex_col()
      .gap_1()
      .children(c.sizes.iter().map(|size| {
        Button::new(format!("cloud-tier-{}", size.id), p)
          .variant(Variant::Outline)
          .size(Size::Sm)
          .w_full()
          .justify_between()
          .selected(size.selected)
          .disabled(c.submitting)
          .aria_label(size.label.clone())
          .child(
            div()
              .flex()
              .gap_2()
              .child(
                div()
                  .font_weight(FontWeight::MEDIUM)
                  .child(if size.current {
                    format!("{} (current)", size.label)
                  } else {
                    size.label.clone()
                  }),
              )
              .child(
                div()
                  .text_color(p.muted_foreground)
                  .child(size.spec.clone()),
              ),
          )
          .children(size.price.clone().map(|price| div().child(price)))
          .on_click(press(&format!("cloud.tier.{}", size.id)))
      }))
  });
  let processes = (!c.processes.is_empty() || c.idle_stop.is_some()).then(|| {
    div()
      .debug_selector(|| "cloud-processes".into())
      .flex()
      .flex_col()
      .gap_1()
      .when_some(c.idle_stop.clone(), |this, eta| {
        this.child(
          // Short label (['countdown']): 11px is fine.
          // ast-grep-ignore: gpui-text-xs-short-label-only
          ui::text(div(), TextSize::Xs)
            .debug_selector(|| "cloud-idle-stop".into())
            .text_color(p.muted_foreground)
            .child(eta),
        )
      })
      .children(c.processes.iter().map(|row| {
        let kill = row.killable.then(|| {
          Button::new(format!("cloud-process-kill-{}", row.pid), p)
            .variant(Variant::Ghost)
            .size(Size::Sm)
            .disabled(c.submitting)
            .label("Kill")
            .on_click(press(&format!("cloud.process.kill.{}", row.pid)))
        });
        let keep = match (&row.pin_id, row.killable) {
          (Some(pin_id), _) => Some(
            Button::new(format!("cloud-process-unpin-{}", row.pid), p)
              .variant(Variant::Outline)
              .size(Size::Sm)
              .selected(true)
              .label("Kept alive")
              .on_click(press(&format!("cloud.process.unpin.{pin_id}"))),
          ),
          (None, true) => Some(
            Button::new(format!("cloud-process-pin-{}", row.pid), p)
              .variant(Variant::Outline)
              .size(Size::Sm)
              .label("Keep alive (billed)")
              .on_click(press(&format!("cloud.process.pin.{}", row.pid))),
          ),
          (None, false) => None,
        };
        ui::text(div(), TextSize::Sm)
          .flex()
          .items_center()
          .gap_2()
          .child(
            div()
              .flex_1()
              .min_w_0()
              .child(div().truncate().child(row.command.clone()))
              .child(
                div()
                  .truncate()
                  .text_color(p.muted_foreground)
                  .child(row.detail.clone()),
              ),
          )
          .children(keep)
          .children(kill)
      }))
  });
  let billing = c.billing.clone().map(|billing| {
    let color = match c.spend_state {
      Some(SpendState::Capped) => p.destructive,
      Some(SpendState::Warning) => Tone::Warning.text(p),
      _ => p.muted_foreground,
    };
    ui::text(div(), TextSize::Sm)
      .debug_selector(|| "cloud-billing".into())
      .flex()
      .flex_col()
      .gap_1()
      .child(div().text_color(color).child(billing))
      .when_some(
        c.spend_note
          .clone()
          .filter(|_| c.spend_state == Some(SpendState::Warning)),
        |this, note| this.child(div().text_color(p.muted_foreground).child(note)),
      )
  });
  // The cap was reached: say why workspaces are stopped and offer to raise it.
  let capped =
    (c.spend_state == Some(SpendState::Capped) || c.spend_cap_reached.is_some()).then(|| {
      let message = c
        .spend_cap_reached
        .clone()
        .or_else(|| c.spend_note.clone())
        .unwrap_or_default();
      div()
        .debug_selector(|| "cloud-spend-capped".into())
        .mt_3()
        .child(
          ui::Alert::new("cloud-spend-capped-alert", "Monthly spend cap reached", p)
            .severity(ui::Severity::Danger)
            .message(message)
            .action(
              Button::new("cloud-spend-cap-raise", p)
                .variant(Variant::Outline)
                .size(Size::Sm)
                .label("Raise cap")
                .on_click(press("cloud.spendCap.edit")),
            ),
        )
    });
  let notices = c.notifications.iter().map(|n| {
    let dismiss = format!("cloud.notification.dismiss.{}", n.id);
    div().mt_3().child(
      ui::Alert::new(
        SharedString::from(format!("cloud-notification-{}", n.id)),
        n.title.clone(),
        p,
      )
      .severity(ui::Severity::Warning)
      .message(n.body.clone())
      .action(
        Button::new(
          SharedString::from(format!("cloud-notification-dismiss-{}", n.id)),
          p,
        )
        .variant(Variant::Ghost)
        .size(Size::Sm)
        .label("Dismiss")
        .on_click(press(&dismiss)),
      ),
    )
  });
  let resize = (c.resize && c.resize_confirm.is_none()).then(|| {
    Button::new("cloud-resize", p)
      .variant(Variant::Outline)
      .size(Size::Sm)
      .disabled(c.submitting)
      .label("Resize")
      .on_click(press("cloud.resize"))
  });
  let migration = c.migration.clone().map(|m| {
    div()
      .mt_3()
      .debug_selector(|| "cloud-migration-alert".into())
      .child(
        ui::Alert::new("cloud-migration", m.title, p)
          .severity(if m.stuck {
            ui::Severity::Warning
          } else {
            ui::Severity::Info
          })
          .message(m.detail.unwrap_or_default()),
      )
  });
  let resize_confirm = c.resize_confirm.clone().map(|lines| {
    div().debug_selector(|| "cloud-resize-confirm-alert".into()).child(
      ui::Alert::new("cloud-resize-unpushed", "Resizing loses unpushed work", p)
        .severity(ui::Severity::Warning)
        .message(format!(
          "The workspace moves to a new machine and repositories are cloned again from their remotes. This is lost: {}. Push first, or resize anyway.",
          lines.join("; ")
        ))
        .action(
          Button::new("cloud-resize-cancel", p).variant(Variant::Ghost).size(Size::Sm)
            .label("Cancel")
            .on_click(press("cloud.resize.cancel")),
        )
        .action(
          Button::new("cloud-resize-confirm", p).variant(Variant::Outline).size(Size::Sm)
            .disabled(c.submitting)
            .label("Resize anyway")
            .on_click(press("cloud.resize.confirm")),
        ),
    )
  });

  let body: AnyElement = match (&c.instance, &c.create) {
    (Some(instance), _) => {
      let usage = instance.usage.as_ref().map(|usage| match usage {
        CloudUsage::Error(error) => ui::text(div(), TextSize::Sm)
          .truncate()
          .text_color(red)
          .child(error.clone())
          .into_any_element(),
        CloudUsage::Loading => ui::Loading::new("cloud-usage-loading", p)
          .label("Loading usage…")
          .into_any_element(),
        CloudUsage::Report {
          summary,
          size,
          percent,
        } => div()
          .flex()
          .flex_col()
          .gap_1p5()
          .child(
            div()
              .flex()
              .items_center()
              .justify_between()
              .text_color(p.muted_foreground)
              .child(summary.clone())
              .child(size.clone()),
          )
          .child(
            div()
              .debug_selector(|| "cloud-disk-usage".into())
              .h(zpx(6.))
              .rounded_full()
              .overflow_hidden()
              .bg(p.muted)
              .child(
                div()
                  .h_full()
                  .w(gpui::relative(*percent as f32 / 100.))
                  .bg(if *percent >= 90 {
                    p.destructive
                  } else {
                    p.primary
                  }),
              ),
          )
          .into_any_element(),
      });
      let open = instance.open_repositories.then(|| {
        Button::new("cloud-open-repositories", p)
          .size(Size::Sm)
          .label("Open repositories")
          .on_click(press("cloud.openRepositories"))
      });
      let wake = instance.wake.then(|| {
        Button::new("cloud-wake", p)
          .size(Size::Sm)
          .disabled(c.submitting)
          .label("Wake")
          .on_click(press("cloud.wake"))
      });
      let deletion = instance.deletion.clone().map(|notice| {
        div()
          .debug_selector(|| "cloud-deletion-warning".into())
          .text_color(p.warning)
          .child(notice)
      });
      let buttons = (open.is_some() || wake.is_some()).then(|| {
        div()
          .flex()
          .flex_wrap()
          .gap_2()
          .children(open)
          .children(wake)
      });
      ui::text(div(), TextSize::Sm)
        .mt_3()
        .flex()
        .flex_col()
        .gap_3()
        .child(
          div()
            .child(
              div()
                .font_weight(FontWeight::MEDIUM)
                .child(instance.label.clone()),
            )
            .when_some(instance.endpoint.clone(), |this, endpoint| {
              this.child(div().text_color(p.muted_foreground).child(endpoint))
            }),
        )
        .children(deletion)
        .children(usage)
        .children(processes)
        .children(sizes)
        .children(resize)
        .children(migration)
        .children(resize_confirm)
        .children(buttons)
        .into_any_element()
    }
    (None, Some(create)) => div()
      .mt_2()
      .flex()
      .flex_col()
      .gap_3()
      .child(
        ui::text(div(), TextSize::Sm)
          .text_color(p.muted_foreground)
          .child("Treq creates one persistent cloud workspace for your account, with Treq, JJ, Git, and agents installed. Pick a size: usage is billed by the second at its hourly price."),
      )
      .children(sizes)
      .child(
        Button::new("cloud-create", p)
          .size(Size::Sm)
          .w_full()
          .disabled(create.disabled)
          .when(create.spinning, |this| {
            this.child(ui::spinner("cloud-create-spinner", 14., p.primary_foreground))
          })
          .label(create.label.clone())
          .on_click(press("cloud.provision")),
      )
      .when_some(create.stage.clone(), |this, stage| {
        this.child(
          ui::text(div(), TextSize::Sm)
            .text_color(p.muted_foreground)
            .child(stage),
        )
      })
      .into_any_element(),
    (None, None) => div().into_any_element(),
  };

  div()
    .debug_selector(|| "cloud-workspace-card".into())
    .rounded(radius::MD)
    .border_1()
    .border_color(p.border)
    .p_4()
    .child(
      ui::text(div(), TextSize::Sm)
        .flex()
        .items_center()
        .gap_2()
        .child(ui::icon(theme::ICON_CLOUD, 16., p.foreground))
        .child(
          div()
            .font_weight(FontWeight::MEDIUM)
            .child("Cloud workspace"),
        )
        .children(actions),
    )
    .child(body)
    .when_some(billing, |this, billing| {
      this.child(div().mt_3().child(billing))
    })
    .children(capped)
    .children(notices)
    .when_some(c.pro_required.clone(), |this, message| {
      this.child(
        ui::text(div(), TextSize::Sm)
          .mt_3()
          .flex()
          .items_center()
          .gap_2()
          .child(ui::Badge::new("PRO", p))
          .child(
            div()
              .flex_1()
              .min_w_0()
              .text_color(p.muted_foreground)
              .child(message),
          )
          .child(
            Button::new("cloud-upgrade", p)
              .variant(Variant::Outline)
              .size(Size::Sm)
              .label("Upgrade to Pro")
              .on_click(press("cloud.upgrade")),
          ),
      )
    })
    .when_some(c.credential_invalid.clone(), |this, message| {
      this.child(
        ui::text(div().mt_3(), TextSize::Sm)
          .debug_selector(|| "cloud-credential-invalid".into())
          .text_color(red)
          .child(message),
      )
    })
    .when_some(c.error.clone(), |this, error| {
      this.child(
        ui::text(div().mt_3(), TextSize::Sm)
          .id("cloud-error")
          .role(gpui::Role::Alert)
          .aria_label(SharedString::from(error.clone()))
          .truncate()
          .text_color(red)
          .child(error)
          .test_support(),
      )
    })
    .when_some(c.error_details.clone(), |this, details| {
      this.child(
        ui::text(div().mt_1(), TextSize::Sm)
          .truncate()
          .text_color(p.muted_foreground)
          .child(details),
      )
    })
    .when(c.sign_in, |this| {
      this.child(
        div().mt_2().child(
          Button::new("cloud-sign-in", p)
            .variant(Variant::Outline)
            .size(Size::Sm)
            .label("Sign in")
            .on_click(press("account.signIn")),
        ),
      )
    })
}

/// The cloud workspace's delete confirmation (`AlertDialog`), pressing
/// `cloud.delete.cancel` and `cloud.delete.confirm`.
pub(in crate::gpui_shell::screen) fn cloud_delete_dialog_element(
  p: &Palette,
  press: PressFn,
) -> impl IntoElement {
  div().absolute().inset_0().child(
    ui::ConfirmDialog::new(
      "cloud-delete-dialog",
      "Delete cloud workspace?",
      "Delete",
      p,
    )
    .message("This permanently deletes the cloud machine and all data on it, including cloned repositories, uncommitted changes, and agent sessions. This cannot be undone.")
    .destructive()
    .on_cancel(handler(press("cloud.delete.cancel")))
    .on_confirm(handler(press("cloud.delete.confirm"))),
  )
}

/// A button's press as a dialog handler, for Escape and Enter.
pub(in crate::gpui_shell::screen) fn handler(
  click: ClickFn,
) -> impl Fn(&mut Window, &mut gpui::App) + 'static {
  move |window, cx| click(&gpui::ClickEvent::default(), window, cx)
}

impl SettingsView {
  pub(super) fn account(
    &mut self,
    tab: &AccountTab,
    p: &Palette,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> AnyElement {
    match tab {
      AccountTab::Loading => centered(0.)
        .child(ui::Loading::new("account-loading", p).label("Loading account…"))
        .into_any_element(),
      AccountTab::SigningIn => centered(12.)
        .debug_selector(|| "account-signing-in".into())
        .child(ui::spinner(
          "account-signing-in-spinner",
          24.,
          p.muted_foreground,
        ))
        .child(
          ui::text(div(), TextSize::Sm)
            .text_color(p.muted_foreground)
            .child("Signing you in…"),
        )
        .into_any_element(),
      AccountTab::SignedOut {
        error,
        dev,
        farewell,
      } => {
        let dev = dev
          .as_ref()
          .map(|dev| self.dev_callback(dev, p, window, cx));
        let farewell = farewell.then(|| {
          div().w_full().max_w(zpx(448.)).child(
            ui::Alert::new("account-farewell", "Sorry to see you go", p)
              .message("Your Treq account and its data have been deleted. You can create a new account anytime.")
              .action(
                Button::new("account-farewell-dismiss", p)
                  .variant(Variant::Outline)
                  .size(Size::Sm)
                  .label("Dismiss")
                  .on_click(self.press("account.farewell.dismiss")),
              ),
          )
        });
        centered(16.)
          .children(farewell)
          .child(
            div()
              .size(zpx(64.))
              .rounded_full()
              .bg(p.muted)
              .flex()
              .items_center()
              .justify_center()
              .child(ui::icon(theme::ICON_USER, 32., p.muted_foreground)),
          )
          .child(
            div()
              .flex()
              .flex_col()
              .items_center()
              .child(heading("Sign in to Treq"))
              .child(help(
                "Sync your settings and manage your subscription",
                p,
                4.,
              )),
          )
          .child(sign_in_button("account-sign-in", self, p))
          .when_some(error.clone(), |this, error| {
            this.child(
              ui::text(div(), TextSize::Sm)
                .text_center()
                .text_color(p.destructive)
                .child(error),
            )
          })
          .children(dev)
          .into_any_element()
      }
      AccountTab::SignedIn(signed_in) => {
        self.signed_in(signed_in, p, window, cx).into_any_element()
      }
    }
  }

  /// The dev build's "Paste callback URL" box.
  fn dev_callback(
    &mut self,
    dev: &DevCallback,
    p: &Palette,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> impl IntoElement {
    let input = self.controls.input(
      "account.callbackUrl",
      &dev.url,
      "treq://auth/callback?token=…",
      window,
      cx,
    );
    div()
      .w_full()
      .max_w(zpx(384.))
      .mt_4()
      .p_3()
      .border_1()
      .border_dashed()
      .border_color(Tone::Warning.border(p))
      .rounded(radius::MD)
      .bg(Tone::Warning.tint(p))
      .child(
        // Short label (['section heading']): 11px is fine.
        // ast-grep-ignore: gpui-text-xs-short-label-only
        ui::text(div(), TextSize::Xs)
          .mb_2()
          .font_weight(FontWeight::MEDIUM)
          .text_color(Tone::Warning.text(p))
          .child("Dev: Paste callback URL"),
      )
      .child(
        div()
          .flex()
          .gap_2()
          .child(form::input_field(
            "account-dev-callback",
            &input,
            true,
            false,
          ))
          .child(
            Button::new("account-dev-callback-go", p)
              .size(Size::Sm)
              .variant(Variant::Outline)
              .disabled(dev.loading || dev.url.trim().is_empty())
              .label(if dev.loading { "…" } else { "Go" })
              .on_click(self.press("account.devCallback.go")),
          ),
      )
      .when_some(dev.error.clone(), |this, error| {
        this.child(
          ui::text(div().mt_1(), TextSize::Sm)
            .text_color(Tone::Danger.color(p))
            .child(error),
        )
      })
  }

  /// A field `width` wide with Cancel and Save (`{id}-cancel`, `{id}-save`),
  /// pressing `{action}.cancel` and `{action}.save`.
  fn field_with_save(
    &self,
    (id, action): (&str, &str),
    field: impl IntoElement,
    width: f32,
    can_save: bool,
    p: &Palette,
  ) -> gpui::Div {
    div()
      .flex()
      .flex_none()
      .gap_2()
      .child(div().w(zpx(width)).child(field))
      .child(
        Button::new(format!("{id}-cancel"), p)
          .variant(Variant::Ghost)
          .size(Size::Sm)
          .label("Cancel")
          .on_click(self.press(&format!("{action}.cancel"))),
      )
      .child(
        Button::new(format!("{id}-save"), p)
          .variant(Variant::Default)
          .size(Size::Sm)
          .disabled(!can_save)
          .label("Save")
          .on_click(self.press(&format!("{action}.save"))),
      )
  }

  /// "Provider keys": bring your own sandbox. Keys go to the server, which
  /// checks them with the provider; they are never shown again.
  fn provider_keys(
    &mut self,
    keys: &ProviderKeysSection,
    p: &Palette,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> AnyElement {
    let editor = keys.editing.as_ref().map(|kind| {
      let input = self
        .controls
        .password("byos.key", &keys.draft, "API key", window, cx);
      if self.account.byos_key_focus {
        self.account.byos_key_focus = false;
        input.read(cx).focus_handle(cx).focus(window, cx);
      }
      let name = crate::core::cloud_catalog::provider_name(kind);
      setting_row(
        &format!("{name} API key"),
        crate::core::cloud_catalog::byos_key_hint(kind),
        self.field_with_save(
          ("byos-key", "byos.key"),
          form::input_field("byos-key-input", &input, false, keys.saving),
          256.,
          !keys.saving && !keys.draft.trim().is_empty(),
          p,
        ),
        p,
      )
    });
    let rows = keys.rows.iter().map(|row| {
      let create = row.can_create.then(|| {
        Button::new(format!("byos-key-create-{}", row.kind), p)
          .variant(Variant::Outline)
          .size(Size::Sm)
          .disabled(keys.saving)
          .label("Create workspace on this account")
          .on_click(self.press(&format!("byos.create.{}", row.kind)))
      });
      let remove = Button::new(format!("byos-key-remove-{}", row.kind), p)
        .variant(Variant::Outline)
        .size(Size::Sm)
        .disabled(keys.saving)
        .label("Remove")
        .on_click(self.press(&format!("byos.key.remove.{}", row.kind)));
      setting_row(
        &row.name,
        &row.status,
        div().flex().gap_2().children(create).child(remove),
        p,
      )
      .when(row.invalid, |this| this.text_color(red(p)))
    });
    let add = div()
      .flex()
      .flex_wrap()
      .gap_2()
      .children(keys.addable.iter().map(|(kind, name)| {
        Button::new(format!("byos-key-add-{kind}"), p)
          .variant(Variant::Outline)
          .size(Size::Sm)
          .disabled(keys.saving)
          .label(format!("Add {name} key"))
          .on_click(self.press(&format!("byos.key.add.{kind}")))
      }));
    div()
      .id("byos-provider-keys")
      .debug_selector(|| "byos-provider-keys".into())
      .flex()
      .flex_col()
      .gap_2()
      .child(ui::SectionHeader::new("provider-keys-heading", "Provider keys", p).icon(theme::ICON_CLOUD).note("Bring your own sandbox"))
      .child(
        ui::text(div(), TextSize::Sm)
          .text_color(p.muted_foreground)
          .child("Run your cloud workspace on your own provider account. The provider bills you for the machine; treq charges 5% of the provider's list price."),
      )
      .children(rows)
      .children(editor)
      .child(add)
      .when_some(keys.error.clone(), |this, error| {
        this.child(ui::text(div(), TextSize::Sm).text_color(red(p)).child(error))
      })
      .into_any_element()
  }

  /// "Monthly spend cap: $50.00" with Change, or the amount field.
  /// "Cloud billing": personal or a Team organization, for the next
  /// workspace.
  fn team_profile(&mut self, section: &CloudProfileSection, p: &Palette) -> AnyElement {
    div()
      .id("team-profile")
      .debug_selector(|| "team-profile".into())
      .flex()
      .flex_col()
      .gap_2()
      .child(
        ui::SectionHeader::new("cloud-billing-heading", "Cloud billing", p)
          .icon(theme::ICON_CLOUD)
          .note("Who pays for your next workspace"),
      )
      .children(section.options.iter().map(|option| {
        let key = option.org_id.clone().unwrap_or_else(|| "personal".into());
        setting_row(
          &option.label,
          &option.note,
          Button::new(format!("team-profile-{key}"), p)
            .variant(Variant::Outline)
            .size(Size::Sm)
            .selected(option.selected)
            .disabled(section.saving || option.selected)
            .label(if option.selected { "In use" } else { "Use" })
            .on_click(self.press(&format!("team.profile.{key}"))),
          p,
        )
      }))
      .children(
        section
          .error
          .clone()
          .map(|e| ui::text(div(), TextSize::Sm).text_color(red(p)).child(e)),
      )
      .child(
        ui::text(div(), TextSize::Sm)
          .text_color(p.muted_foreground)
          .child("A workspace keeps the billing it was created with."),
      )
      .into_any_element()
  }

  /// An owner's cloud settings for one Team organization: what members may
  /// run, both caps, and the organization's provider keys.
  fn org_cloud(
    &mut self,
    org: &OrgCloudSection,
    p: &Palette,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> AnyElement {
    let id = org.org_id.clone();
    let org_cap = self.controls.input(
      &format!("team.cap.org.{id}"),
      &org.org_cap,
      "500",
      window,
      cx,
    );
    let member_cap = self.controls.input(
      &format!("team.cap.member.{id}"),
      &org.member_cap,
      "50",
      window,
      cx,
    );
    let editor = org.editing.as_ref().map(|kind| {
      let input = self
        .controls
        .password("team.key", &org.draft, "API key", window, cx);
      if self.account.team_key_focus {
        self.account.team_key_focus = false;
        input.read(cx).focus_handle(cx).focus(window, cx);
      }
      let name = crate::core::cloud_catalog::provider_name(kind);
      setting_row(
        &format!("{name} API key for {}", org.name),
        "Checked with the provider before it is saved.",
        self.field_with_save(
          ("team-key", "team.key"),
          form::input_field("team-key-input", &input, false, org.saving),
          256.,
          !org.saving && !org.draft.trim().is_empty(),
          p,
        ),
        p,
      )
    });
    let keys = org.allow_byos.then(|| {
      div()
        .flex()
        .flex_col()
        .gap_2()
        .children(org.keys.iter().map(|row| {
          let create = row.can_create.then(|| {
            Button::new(format!("team-{id}-key-create-{}", row.kind), p)
              .variant(Variant::Outline)
              .size(Size::Sm)
              .disabled(org.saving)
              .label("Create workspace on this account")
              .on_click(self.press(&format!("byos.create.{}", row.kind)))
          });
          let remove = Button::new(format!("team-{id}-key-remove-{}", row.kind), p)
            .variant(Variant::Outline)
            .size(Size::Sm)
            .disabled(org.saving)
            .label("Remove")
            .on_click(self.press(&format!("team.key.remove.{id}.{}", row.kind)));
          setting_row(
            &row.name,
            &row.status,
            div().flex().gap_2().children(create).child(remove),
            p,
          )
          .when(row.invalid, |this| this.text_color(red(p)))
        }))
        .child(
          div()
            .flex()
            .flex_wrap()
            .gap_2()
            .children(org.addable.iter().map(|(kind, name)| {
              Button::new(format!("team-{id}-key-add-{kind}"), p)
                .variant(Variant::Outline)
                .size(Size::Sm)
                .disabled(org.saving)
                .label(format!("Add {name} key"))
                .on_click(self.press(&format!("team.key.add.{id}.{kind}")))
            })),
        )
    });
    div()
      .id(SharedString::from(format!("team-{id}")))
      .debug_selector({
        let id = id.clone();
        move || format!("team-{id}")
      })
      .flex()
      .flex_col()
      .gap_2()
      .child(ui::SectionHeader::new("organization-cloud-heading", "Organization cloud", p).icon(theme::ICON_CLOUD).note(format!("{}, owners only", org.name)))
      .child(setting_row(
        "treq-managed sandboxes",
        "Members run on treq's account. Billed to the organization at cost plus 20%.",
        form::switch(
          format!("team-{id}-managed"),
          "treq-managed sandboxes",
          org.allow_managed,
          org.saving,
          p,
          self.field(&format!("team.managed.{id}")),
        ),
        p,
      ))
      .child(setting_row(
        "The organization's own keys",
        "Members run on the organization's provider accounts. treq bills the organization 5% of the provider's price.",
        form::switch(
          format!("team-{id}-byos"),
          "The organization's own keys",
          org.allow_byos,
          org.saving,
          p,
          self.field(&format!("team.byos.{id}")),
        ),
        p,
      ))
      .child(setting_row(
        "Organization cap",
        "Monthly, for everyone together. In US dollars.",
        form::input_field(format!("team-{id}-org-cap"), &org_cap, false, org.saving),
        p,
      ))
      .child(setting_row(
        "Member cap",
        "Monthly, for each member. In US dollars.",
        form::input_field(format!("team-{id}-member-cap"), &member_cap, false, org.saving),
        p,
      ))
      .child(
        div().flex().child(
          Button::new(format!("team-{id}-save"), p)
            .size(Size::Sm)
            .disabled(org.saving)
            .label("Save")
            .on_click(self.press(&format!("team.save.{id}"))),
        ),
      )
      .children(keys)
      .children(editor)
      .children(
        org
          .error
          .clone()
          .map(|e| ui::text(div(), TextSize::Sm).text_color(red(p)).child(e)),
      )
      .into_any_element()
  }

  fn spend_cap_row(
    &mut self,
    editor: &SpendCapEditor,
    p: &Palette,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> AnyElement {
    let control: AnyElement = if editor.editing {
      let input = self
        .controls
        .input("cloud.spendCap", &editor.draft, "50", window, cx);
      if self.account.spend_cap_focus {
        self.account.spend_cap_focus = false;
        input.read(cx).focus_handle(cx).focus(window, cx);
      }
      self
        .field_with_save(
          ("cloud-spend-cap", "cloud.spendCap"),
          form::input_field("cloud-spend-cap-input", &input, false, false),
          120.,
          true,
          p,
        )
        .into_any_element()
    } else {
      Button::new("cloud-spend-cap-edit", p)
        .variant(Variant::Outline)
        .size(Size::Sm)
        .label("Change")
        .on_click(self.press("cloud.spendCap.edit"))
        .into_any_element()
    };
    div()
      .debug_selector(|| "cloud-spend-cap".into())
      .flex()
      .flex_col()
      .gap_1()
      .child(setting_row(
        &editor.label,
        "Cloud workspaces stop when this month's spend reaches the cap. Minimum $5.",
        control,
        p,
      ))
      .when_some(editor.error.clone(), |this, error| {
        this.child(
          ui::text(div(), TextSize::Sm)
            .text_color(p.destructive)
            .child(error),
        )
      })
      .into_any_element()
  }

  fn signed_in(
    &mut self,
    s: &SignedIn,
    p: &Palette,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> impl IntoElement {
    let avatar = self
      .account
      .avatar
      .as_ref()
      .and_then(|(_, image)| image.clone());
    let avatar: AnyElement = match avatar {
      Some(image) => gpui::img(image)
        .size(zpx(48.))
        .flex_none()
        .rounded_full()
        .into_any_element(),
      None => div()
        .size(zpx(48.))
        .flex_none()
        .rounded_full()
        .bg(p.muted)
        .flex()
        .items_center()
        .justify_center()
        .child(ui::icon(theme::ICON_USER, 24., p.muted_foreground))
        .into_any_element(),
    };
    let card = || {
      div()
        .border_1()
        .border_color(p.border)
        .rounded(radius::MD)
        .p_4()
    };
    let row = |name: String, value: AnyElement| {
      ui::text(div(), TextSize::Sm)
        .flex()
        .items_center()
        .justify_between()
        .child(div().text_color(p.muted_foreground).child(name))
        .child(value)
    };
    let user = card().child(
      div().flex().items_center().gap_4().child(avatar).child(
        ui::text(div(), TextSize::Sm)
          .flex_1()
          .min_w_0()
          .child(
            div()
              .font_weight(FontWeight::MEDIUM)
              .truncate()
              .child(s.name.clone()),
          )
          .child(
            div()
              .text_color(p.muted_foreground)
              .truncate()
              .child(s.email.clone().unwrap_or_default()),
          ),
      ),
    );
    let plan = div()
      .flex()
      .items_center()
      .gap_1()
      .font_weight(FontWeight::MEDIUM)
      .when(s.pro, |this| {
        this.child(ui::icon(theme::ICON_CROWN, 12., Tone::Warning.color(p)))
      })
      .child(if s.pro { "Pro" } else { "Free" });
    let subscription = card()
      .child(
        ui::text(div(), TextSize::Sm)
          .flex()
          .items_center()
          .gap_2()
          .mb_3()
          .child(ui::icon(theme::ICON_CREDIT_CARD, 16., p.foreground))
          .child(div().font_weight(FontWeight::MEDIUM).child("Subscription")),
      )
      .child(
        div()
          .flex()
          .flex_col()
          .gap_2()
          .child(row("Plan".into(), plan.into_any_element()))
          .when_some(s.status.clone(), |this, (status, tone)| {
            let color = match tone {
              StatusTone::Good => Tone::Success.text(p),
              StatusTone::Warn => Tone::Warning.text(p),
              StatusTone::Muted => p.muted_foreground,
            };
            this.child(row(
              "Status".into(),
              div()
                .font_weight(FontWeight::MEDIUM)
                .text_color(color)
                .child(status)
                .into_any_element(),
            ))
          })
          .when_some(s.period.clone(), |this, (label, date)| {
            this.child(row(label, div().child(date).into_any_element()))
          }),
      )
      .child(
        div().mt_4().child(
          Button::new("account-manage-subscription", p)
            .variant(Variant::Outline)
            .size(Size::Sm)
            .w_full()
            .child(ui::icon(theme::ICON_EXTERNAL_LINK, 12., p.foreground))
            .label(s.manage_label.clone())
            .on_click(self.press("account.manage")),
        ),
      );
    let cloud = s.cloud.as_ref().map(|c| self.cloud_card(c, p, cx));
    let spend_cap = s
      .spend_cap
      .as_ref()
      .map(|editor| self.spend_cap_row(editor, p, window, cx));
    let provider_keys = s
      .provider_keys
      .as_ref()
      .map(|keys| self.provider_keys(keys, p, window, cx));
    let team_profile = s
      .cloud_profile
      .as_ref()
      .map(|section| self.team_profile(section, p));
    let org_cloud: Vec<AnyElement> = s
      .org_cloud
      .iter()
      .map(|org| self.org_cloud(org, p, window, cx))
      .collect();
    div()
      .flex()
      .flex_col()
      .gap_6()
      .child(user)
      .child(subscription)
      .children(cloud)
      .children(spend_cap)
      .children(team_profile)
      .children(org_cloud)
      .children(provider_keys)
      .child(
        div().flex().child(
          Button::new("account-sign-out", p)
            .variant(Variant::Outline)
            .size(Size::Sm)
            .text_color(p.destructive)
            .child(ui::icon(theme::ICON_LOG_OUT, 16., p.destructive))
            .label("Sign out")
            .on_click(self.press("account.signOut")),
        ),
      )
      .child(
        div()
          .id("account-delete-section")
          .flex()
          .flex_col()
          .gap_1()
          .p_4()
          .rounded(radius::MD)
          .border_1()
          .border_color(Tone::Danger.border(p))
          .child(ui::text(div(), TextSize::Sm).font_weight(FontWeight::MEDIUM).child("Delete account"))
          .child(
            ui::text(div(), TextSize::Sm)
              .text_color(p.muted_foreground)
              .child("Permanently deletes your Treq account, cancels any subscription immediately, and deletes your cloud workspace."),
          )
          .child(
            div().mt_2().flex().child(
              Button::new("account-delete", p)
                .variant(Variant::Outline)
                .size(Size::Sm)
                .text_color(p.destructive)
                .child(ui::icon(theme::ICON_TRASH, 16., p.destructive))
                .label("Delete account")
                .on_click(self.press("account.delete.ask")),
            ),
          ),
      )
  }

  /// "Delete your Treq account?", typed-email confirmation.
  pub(super) fn delete_account_dialog(
    &mut self,
    dialog: &DeleteAccountDialog,
    email: &str,
    p: &Palette,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> impl IntoElement {
    let input = self
      .controls
      .input("account.delete.email", &dialog.typed, email, window, cx);
    let can_confirm = dialog.can_confirm;
    div().absolute().inset_0().child(
      ui::Dialog::new("account-delete-dialog", p)
        .width(512.)
        .title("Delete your Treq account?")
        .description("This cannot be undone. Deleting your account:")
        .on_close(handler(Box::new(self.press("account.delete.cancel"))))
        .on_primary(handler(Box::new(self.press("account.delete.confirm"))))
        .primary_disabled(!can_confirm || dialog.deleting)
          .child(
            div()
              .flex()
              .flex_col()
              .gap_2()
              .child(
                div().flex().flex_col().gap_1().children(
                  [
                    "Cancels your subscription immediately, without a refund for the rest of the period.",
                    "Deletes your cloud workspace and everything on it.",
                    "Disconnects Linear and forgets your GitHub App installations.",
                    "Keeps billing records for tax and accounting.",
                  ]
                  .map(|item| {
                    div()
                      .flex()
                      .gap_2()
                      .child(ui::text(div().flex_none(), TextSize::Sm).text_color(p.muted_foreground).child("•"))
                      .child(ui::text(div().min_w_0(), TextSize::Sm).text_color(p.muted_foreground).child(item))
                  }),
                ),
              )
              .child(
                ui::text(div(), TextSize::Sm)
                  .text_color(p.muted_foreground)
                  .child("If you are the last owner of an organization, make another member an owner or delete the organization first."),
              ),
          )
          .child(
            div()
              .flex()
              .flex_col()
              .gap_2()
              .child(
                ui::text(div(), TextSize::Sm)
                  .font_weight(FontWeight::MEDIUM)
                  .child(format!("Type {email} to confirm")),
              )
              .child(form::input_field(
                "account-delete-email",
                &input,
                false,
                dialog.deleting,
              ))
              .when_some(dialog.error.clone(), |this, error| {
                this.child(
                  ui::text(div(), TextSize::Sm)
                    .id("account-delete-error")
                    .role(gpui::Role::Alert)
                    .aria_label(SharedString::from(error.clone()))
                    .text_color(Tone::Danger.color(p))
                    .child(error)
                    .test_support(),
                )
              }),
          )
          .confirm()
          .footer([
            Button::new("account-delete-cancel", p)
              .variant(Variant::Outline)
              .disabled(dialog.deleting)
              .label("Cancel")
              .on_click(self.press("account.delete.cancel"))
              .into_any_element(),
            Button::new("account-delete-confirm", p)
              .variant(Variant::Destructive)
              .disabled(!can_confirm)
              .label(if dialog.deleting { "Deleting…" } else { "Delete account" })
              .on_click(self.press("account.delete.confirm"))
              .into_any_element(),
          ]),
    )
  }

  /// `CloudWorkspaceCard`.
  fn cloud_card(&mut self, c: &CloudCard, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
    cloud_card_element(c, p, &self.tips, &|id| Box::new(self.press(id)), cx)
  }

  /// The cloud workspace's delete confirmation (`AlertDialog`).
  pub(super) fn cloud_delete_dialog(&self, p: &Palette) -> impl IntoElement {
    cloud_delete_dialog_element(p, &|id| Box::new(self.press(id)))
  }

  pub(super) fn integrations(
    &mut self,
    tab: &IntegrationsTab,
    p: &Palette,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> impl IntoElement {
    let open = |id: &str| tab.expanded.iter().any(|e| e == id);
    let github = self.integration_card(
      IntegrationCard {
        id: "github",
        icon: theme::ICON_GITHUB,
        name: "GitHub",
        purpose: "Pull requests, checks and reviews for your repositories",
        connected: tab.github_connected,
        open: open("github"),
      },
      open("github").then(|| self.github(&tab.github, p)),
      p,
    );
    let linear = tab.linear.as_ref().map(|linear| {
      let body = open("linear").then(|| self.linear(linear, p, window, cx));
      self.integration_card(
        IntegrationCard {
          id: "linear",
          icon: theme::ICON_LINEAR,
          name: "Linear",
          purpose: "Issues to start workspaces from",
          connected: linear.connected,
          open: open("linear"),
        },
        body,
        p,
      )
    });
    div()
      .flex()
      .flex_col()
      .gap_3()
      .child(github)
      .children(linear)
  }

  /// One integration's card: a header row (icon, name, purpose, status,
  /// chevron) that expands the card to configure it.
  fn integration_card(
    &self,
    card: IntegrationCard,
    body: Option<AnyElement>,
    p: &Palette,
  ) -> gpui::Div {
    let status = div()
      .debug_selector({
        let id = format!("integration-status-{}", card.id);
        move || id
      })
      .flex()
      .items_center()
      .gap_1p5()
      .child(ui::Dot::new(
        if card.connected {
          Tone::Success
        } else {
          Tone::Neutral
        },
        if card.connected {
          "Connected"
        } else {
          "Not connected"
        },
        p,
      ))
      .child(
        ui::text(div(), TextSize::Sm)
          .text_color(p.muted_foreground)
          .child(if card.connected {
            "Connected"
          } else {
            "Not connected"
          }),
      );
    let header = ui::Row::new(format!("integration-card-{}", card.id), p)
      .role(gpui::Role::Button)
      .label(format!("{} integration", card.name))
      .density(ui::RowDensity::TwoLine)
      .leading(ui::icon(card.icon, 20., p.foreground))
      .title(card.name)
      .meta(card.purpose)
      .trailing(status)
      .trailing(ui::icon(
        if card.open {
          theme::ICON_CHEVRON_UP
        } else {
          theme::ICON_CHEVRON_DOWN
        },
        16.,
        p.muted_foreground,
      ))
      .px_4()
      .on_click(self.press(&format!("integration.toggle.{}", card.id)));
    ui::card(p)
      .overflow_hidden()
      .child(header)
      .when_some(body, |this, body| {
        this.child(
          div()
            .border_t_1()
            .border_color(p.border)
            .px_4()
            .py_2()
            .child(body),
        )
      })
  }

  fn github(&self, github: &GithubSection, p: &Palette) -> AnyElement {
    match github {
      GithubSection::Loading => div()
        .flex()
        .justify_center()
        .py_6()
        .child(ui::Loading::new("github-loading", p).label("Loading GitHub…"))
        .into_any_element(),
      GithubSection::SignedOut => div()
        .flex()
        .items_center()
        .justify_between()
        .gap_4()
        .py_3()
        .child(
          ui::text(div(), TextSize::Sm)
            .text_color(p.muted_foreground)
            .child("Sign in to Treq Cloud to connect GitHub"),
        )
        .child(sign_in_button("integrations-sign-in", self, p))
        .into_any_element(),
      GithubSection::SignedIn {
        scope,
        description,
        repositories,
      } => {
        let manage = Button::new("github-manage", p)
          .variant(Variant::Outline)
          .size(Size::Sm)
          .child(ui::icon(theme::ICON_EXTERNAL_LINK, 12., p.foreground))
          .label("Manage GitHub")
          .on_click(self.press("github.manage"));
        div()
          .child(
            ui::SectionHeader::new("github-heading", "GitHub", p)
              .icon(theme::ICON_GITHUB)
              .note(scope),
          )
          .child(setting_row(
            "Connected repositories",
            description,
            manage,
            p,
          ))
          .when(!repositories.is_empty(), |this| {
            this.child(
              ui::text(div(), TextSize::Sm)
                .debug_selector(|| "github-repositories".into())
                .border_t_1()
                .border_color(p.border)
                .py_2()
                .children(repositories.iter().map(|(name, private)| {
                  div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .py_1p5()
                    .child(div().font_family(theme::MONO_FONT).child(name.clone()))
                    .child(div().text_color(p.muted_foreground).child(if *private {
                      "Private"
                    } else {
                      "Public"
                    }))
                })),
            )
          })
          .into_any_element()
      }
    }
  }

  fn linear(
    &mut self,
    linear: &LinearSection,
    p: &Palette,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> AnyElement {
    if !linear.repo {
      return div()
        .text_center()
        .py_8()
        .text_color(p.muted_foreground)
        .child("Select a repository to configure Linear integration")
        .into_any_element();
    }
    if linear.upgrade {
      let button = Button::new("linear-upgrade", p)
        .size(Size::Sm)
        .label("Upgrade to Pro")
        .on_click(self.press("cloud.upgrade"));
      return setting_row("Linear needs Pro", "Upgrade to use Linear", button, p)
        .into_any_element();
    }
    let key_control: AnyElement = if linear.editing {
      let input = self
        .controls
        .password("linear.apiKey", &linear.draft, "lin_api_…", window, cx);
      if let Some(state) = self.account.linear.as_mut().filter(|l| l.focus) {
        state.focus = false;
        input.read(cx).focus_handle(cx).focus(window, cx);
      }
      div()
        .flex()
        .flex_none()
        .gap_2()
        .child(div().w(zpx(256.)).child(form::input_field(
          "linear-api-key-input",
          &input,
          false,
          linear.saving,
        )))
        .child(
          Button::new("linear-api-key-cancel", p)
            .size(Size::Sm)
            .variant(Variant::Ghost)
            .disabled(linear.saving)
            .label("Cancel")
            .on_click(self.press("linear.apiKey.cancel")),
        )
        .child(
          Button::new("linear-api-key-save", p)
            .size(Size::Sm)
            .disabled(linear.saving || linear.draft.is_empty())
            .when(linear.saving, |this| {
              this.child(ui::spinner(
                "linear-api-key-saving",
                14.,
                p.primary_foreground,
              ))
            })
            .label("Save")
            .on_click(self.press("linear.apiKey.save")),
        )
        .into_any_element()
    } else {
      Button::new("linear-api-key-edit", p)
        .size(Size::Sm)
        .variant(Variant::Outline)
        .label(linear.edit_label.clone())
        .on_click(self.press("linear.apiKey.edit"))
        .into_any_element()
    };
    let oauth = linear.oauth.then(|| {
      let button = Button::new("linear-oauth", p)
        .size(Size::Sm)
        .disabled(linear.oauth_loading)
        .child(if linear.oauth_loading {
          ui::spinner("linear-oauth-loading", 14., p.primary_foreground).into_any_element()
        } else {
          ui::icon(theme::ICON_EXTERNAL_LINK, 12., p.primary_foreground).into_any_element()
        })
        .label("Connect via OAuth")
        .on_click(self.press("linear.oauth"));
      setting_row("OAuth connection", &linear.oauth_description, button, p)
        .border_t_1()
        .border_color(p.border)
    });
    let create_key = Button::new("linear-create-key", p)
      .variant(Variant::Link)
      .size(Size::Sm)
      .child(ui::icon(theme::ICON_EXTERNAL_LINK, 12., p.primary))
      .label("Create a key in Linear")
      .on_click(self.press("linear.createKey"));
    div()
      .child(setting_row("API key", &linear.description, key_control, p))
      .when(!linear.connected, |this| {
        this.child(div().pb_2().flex().child(create_key))
      })
      .children(oauth)
      .when(linear.key_priority_note, |this| {
        this.child(
          div().py_3().border_t_1().border_color(p.border).child(
            ui::text(div(), TextSize::Sm)
              .text_color(p.muted_foreground)
              .child("Direct API key configured. This takes priority over OAuth authentication."),
          ),
        )
      })
      .into_any_element()
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn reads_the_token_from_a_pasted_callback() {
    assert_eq!(
      callback_token("treq://auth/callback?token=a%2Bb"),
      Ok("a+b".into())
    );
    assert_eq!(callback_token("  plain  "), Ok("plain".into()));
    assert_eq!(
      callback_token("not a url token=1"),
      Err("Invalid URL".into())
    );
    assert_eq!(
      callback_token("treq://auth/callback?token="),
      Err("No token found in URL".into())
    );
  }

  #[test]
  fn formats_disk_usage_like_react() {
    assert_eq!(format_bytes(512. * MIB), "512 MB");
    assert_eq!(format_bytes(1.5 * GIB), "1.5 GB");
    assert_eq!(js_number(5.), "5");
    assert_eq!(js_number(2.5), "2.5");
  }

  #[test]
  fn counts_canceling_subscriptions_as_pro() {
    let sub = |plan: &str, status: &str| Subscription {
      plan: plan.into(),
      status: status.into(),
      current_period_end: None,
    };
    assert!(is_pro(Some(&sub("pro", "canceled"))));
    assert!(!is_pro(Some(&sub("pro", "inactive"))));
    assert!(!is_pro(Some(&sub("free", "active"))));
    assert!(!is_pro(None));
  }
}
