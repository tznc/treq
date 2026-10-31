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

use crate::gpui_shell::ui::radius;
use crate::gpui_shell::ui::zpx;
use std::sync::Arc;

use gpui_kit::base::TestSupportExt as _;
use serde::Deserialize;
use tauri_plugin_gpui::gpui::{
  self, div, prelude::*, AnyElement, Context, Focusable as _, FontWeight, SharedString, Window,
};

use super::{help, toast, SettingsView, Tab};
use crate::gpui_shell::{
  form,
  native::{self, NativeEvent, ToastTone},
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
  pub submitting: bool,
  pub confirming_delete: bool,
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
  /// While the Linear integration preview is on.
  pub linear: Option<LinearSection>,
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
  pub description: String,
  pub editing: bool,
  pub draft: String,
  pub saving: bool,
  pub edit_label: String,
  /// The OAuth row shows (Pro).
  pub oauth: bool,
  pub oauth_description: String,
  pub oauth_loading: bool,
  pub key_priority_note: bool,
}

/// The tabs' own state.
#[derive(Default)]
pub(super) struct State {
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
  /// The open delete-account confirmation (`can_confirm` is derived).
  delete_account: Option<DeleteAccountDialog>,
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
/// (supabase/migrations/038_lapse_grace_30_days.sql).
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
    "provisioning" => "Creating cloud workspace...",
    "bootstrapping" => "Installing Treq, JJ, Git, and agents...",
    "installing_access" => "Configuring access...",
    "verifying" => "Verifying readiness...",
    "ready" => "Ready",
    "suspended" => "Suspended (idle)",
    "waking" => "Waking...",
    "reprovisioning" => "Reprovisioning...",
    "degraded" => "Degraded",
    "failed" => "Failed",
    "deleting" => "Deleting...",
    "deleted" => "Deleted",
    other => other,
  }
  .to_string()
}

/// How `remoteFunctionError` stores a refusal for want of Pro.
const PRO_REQUIRED_PREFIX: &str = "[pro_required] ";

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
  let creating = cloud.submitting && instance.is_none();
  let stage = cloud.provisioning_stage.clone().filter(|s| !s.is_empty());
  CloudCard {
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
        "Creating..."
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
    error: error.filter(|_| pro_required.is_none()).map(first_line),
    submitting: cloud.submitting,
    confirming_delete,
  }
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
    AccountTab::SignedIn(Box::new(SignedIn {
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
        "Update Payment Method"
      } else if pro {
        "Manage Subscription"
      } else {
        "Upgrade to Pro"
      }
      .into(),
      cloud: account
        .cloud
        .as_ref()
        .map(|cloud| cloud_card(cloud, state.confirming_delete)),
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
          description: if key.is_some() {
            "Direct API access configured"
          } else {
            "Free plan: provide your API key"
          }
          .into(),
          editing: linear.is_some_and(|l| l.editing),
          draft: linear.map(|l| l.draft.clone()).unwrap_or_default(),
          saving: linear.is_some_and(|l| l.saving),
          edit_label: format!("{} API Key", if key.is_some() { "Update" } else { "Add" }),
          oauth: pro && native::feature_flag("pro"),
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
    IntegrationsTab { github, linear }
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
              host.notify(toast(ToastTone::Success, "API Key saved", ""));
            }
            Err(err) => host.notify(toast(ToastTone::Error, "Error saving API key", err)),
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
          view.host.notify(toast(
            ToastTone::Success,
            "Account deleted",
            "Your Treq account and its data have been deleted.",
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
            host.notify(NativeEvent::Toast {
              title: "Linear OAuth needs Pro".into(),
              description: "Add a personal Linear API key above to use Linear on the Free plan."
                .into(),
              tone: ToastTone::Warning,
              action: Some(native::ToastAction {
                label: "Upgrade to Pro".into(),
                id,
              }),
            });
          }
          Err(error) => host.notify(toast(
            ToastTone::Error,
            "Error starting OAuth flow",
            error.message,
          )),
        }
      },
    );
  }

  /// The account and integrations buttons GPUI handles; false for the rest.
  pub(super) fn press_account(&mut self, id: &str, cx: &mut Context<Self>) -> bool {
    let web = native::web_url();
    match id {
      "account.signIn" => self.host.open_url(&format!("{web}/sign-in?source=desktop")),
      // A failed payment goes straight to the subscription tab, where the
      // billing portal updates the card.
      "account.manage"
        if past_due(
          self
            .account_route()
            .and_then(|a| a.subscription.clone())
            .as_ref(),
        ) =>
      {
        self
          .host
          .open_url(&format!("{web}/dashboard?tab=subscription"))
      }
      "account.manage" | "cloud.upgrade" => self.host.open_url(&format!("{web}/dashboard")),
      "github.manage" => self
        .host
        .open_url(&format!("{web}/dashboard?tab=integrations")),
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
      "cloud.provision" => self.host.cloud_action(CloudAction::Provision),
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
      _ => return false,
    }
    true
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

/// "Sign in with Browser", which opens the website's sign-in page.
fn sign_in_button(id: &'static str, view: &SettingsView, p: &Palette) -> Button {
  Button::new(id, p)
    .child(ui::icon(
      theme::ICON_EXTERNAL_LINK,
      16.,
      p.primary_foreground,
    ))
    .label("Sign in with Browser")
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

/// An integration's header: icon, name and a muted note, over a rule.
fn section_header(icon: &'static str, name: &'static str, note: &str, p: &Palette) -> gpui::Div {
  ui::text(div(), TextSize::Sm)
    .flex()
    .items_center()
    .gap_3()
    .pb_2()
    .border_b_1()
    .border_color(p.border)
    .child(ui::icon(icon, 20., p.foreground))
    .child(div().font_weight(FontWeight::SEMIBOLD).child(name))
    .child(div().text_color(p.muted_foreground).child(note.to_string()))
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

  let body: AnyElement = match (&c.instance, &c.create) {
    (Some(instance), _) => {
      let usage = instance.usage.as_ref().map(|usage| match usage {
        CloudUsage::Error(error) => ui::text(div(), TextSize::Xs)
          .truncate()
          .text_color(red)
          .child(error.clone())
          .into_any_element(),
        CloudUsage::Loading => div()
          .text_color(p.muted_foreground)
          .child("Loading usage...")
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
          .child("Treq creates one persistent cloud workspace for your account, with Treq, JJ, Git, and agents installed. Location, CPU, memory, and storage are managed for you."),
      )
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
    .when_some(c.error.clone(), |this, error| {
      this.child(
        ui::text(div().mt_3(), TextSize::Xs)
          .truncate()
          .text_color(red)
          .child(error),
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
        .child(ui::spinner("account-loading", 24., p.muted_foreground))
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
      AccountTab::SignedIn(signed_in) => self.signed_in(signed_in, p, cx).into_any_element(),
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
      "treq://auth/callback?token=...",
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
              .label(if dev.loading { "..." } else { "Go" })
              .on_click(self.press("account.devCallback.go")),
          ),
      )
      .when_some(dev.error.clone(), |this, error| {
        this.child(
          ui::text(div().mt_1(), TextSize::Xs)
            .text_color(Tone::Danger.color(p))
            .child(error),
        )
      })
  }

  fn signed_in(&mut self, s: &SignedIn, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
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
    div()
      .flex()
      .flex_col()
      .gap_6()
      .child(user)
      .child(subscription)
      .children(cloud)
      .child(
        div().flex().child(
          Button::new("account-sign-out", p)
            .variant(Variant::Outline)
            .size(Size::Sm)
            .text_color(p.destructive)
            .child(ui::icon(theme::ICON_LOG_OUT, 16., p.destructive))
            .label("Sign Out")
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
                .label("Delete Account")
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
              .label(if dialog.deleting { "Deleting…" } else { "Delete Account" })
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
    let linear = tab
      .linear
      .as_ref()
      .map(|linear| self.linear(linear, p, window, cx));
    div()
      .flex()
      .flex_col()
      .gap_12()
      .child(self.github(&tab.github, p))
      .children(linear)
  }

  fn github(&self, github: &GithubSection, p: &Palette) -> AnyElement {
    match github {
      GithubSection::Loading => div()
        .flex()
        .justify_center()
        .py_16()
        .child(ui::spinner("github-loading", 24., p.muted_foreground))
        .into_any_element(),
      GithubSection::SignedOut => centered(16.)
        .text_center()
        .child(ui::icon(theme::ICON_GITHUB, 48., p.muted_foreground))
        .child(
          div()
            .flex()
            .flex_col()
            .items_center()
            .child(heading("Sign in to manage integrations"))
            .child(help(
              "Connect your GitHub repositories through Treq.",
              p,
              4.,
            )),
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
          .child(section_header(theme::ICON_GITHUB, "GitHub", scope, p))
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
    let key_control: AnyElement = if linear.editing {
      let input = self
        .controls
        .password("linear.apiKey", &linear.draft, "lin_api_...", window, cx);
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
      setting_row("OAuth Connect", &linear.oauth_description, button, p)
        .border_t_1()
        .border_color(p.border)
    });
    div()
      .child(section_header(
        theme::ICON_LINEAR,
        "Linear",
        "Issue tracker integration",
        p,
      ))
      .child(setting_row("API Key", &linear.description, key_control, p))
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
