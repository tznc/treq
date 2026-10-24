//! `SettingsPage`: every tab.
//!
//! Drawn from the backend directly: GPUI loads and saves the settings
//! itself, then tells React (see `native`) so its stores and caches follow.
//! React still owns which tab shows, closing the page, and the Supabase
//! session the Account and Integrations tabs show (see `account`).

use crate::gpui_shell::ui::radius;
use crate::gpui_shell::ui::zpx;
use crate::result_ext::ResultExt;
use std::{rc::Rc, sync::Arc, time::Duration};

use gpui_kit::base::TestSupportExt as _;
use serde::Deserialize;
use tauri_plugin_gpui::gpui::{
  self, div, prelude::*, AnyElement, Context, FontWeight, Role, SharedString, Window,
};

pub(super) mod account;

// The tests read what the tabs draw by these names.
#[cfg_attr(
  not(test),
  allow(unused_imports, reason = "re-exported for the tests only")
)]
pub use account::{
  AccountRoute, AccountTab, CloudCard, CloudEndpoint, CloudInstance, CloudRoute, CloudUsage,
  GithubSection, IntegrationsTab, LinearSection, StatusTone, Subscription, UsageReport, User,
};

use super::{
  checklist,
  settings_data::{self as data, AppFields, SettingsHost},
  ActionSink, ScreenAction,
};
use crate::{
  core::{
    app_setup_script::AppSetupScriptStatus, feature_preview::PreviewFeature,
    skills::SkillCatalogSkillView, PrerequisiteStatus,
  },
  gpui_shell::{
    form::{self, Controls, SelectOption},
    native::{self, NativeEvent, ToastTone},
    popup::PopupSink,
    theme::{self, Palette},
    tooltips::{HasTooltips, Tooltips},
    ui::{self, Button, Size, TextSize, Variant},
  },
  repo_config::RepoConfig,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Tab {
  Repository,
  Application,
  Account,
  Integrations,
  Skills,
  Preview,
}

impl Tab {
  fn label(self) -> &'static str {
    match self {
      Tab::Repository => "Repository",
      Tab::Application => "Application",
      Tab::Account => "Account",
      Tab::Integrations => "Integrations",
      Tab::Skills => "Skills",
      Tab::Preview => "Feature Preview",
    }
  }

  fn icon(self) -> &'static str {
    match self {
      Tab::Repository => theme::ICON_FOLDER_GIT,
      Tab::Application => theme::ICON_SETTINGS,
      Tab::Account => theme::ICON_USER,
      Tab::Integrations => theme::ICON_PLUG,
      Tab::Skills => theme::ICON_BOOK_OPEN,
      Tab::Preview => theme::ICON_FLASK,
    }
  }

  fn value(self) -> &'static str {
    match self {
      Tab::Repository => "repository",
      Tab::Application => "application",
      Tab::Account => "account",
      Tab::Integrations => "integrations",
      Tab::Skills => "skills",
      Tab::Preview => "preview",
    }
  }
}

/// What React sends: the tab it shows and the repository in view.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Route {
  pub tab: Tab,
  pub repo_path: Option<String>,
  /// The home repository's branch, as the workspace header shows it.
  pub current_branch: Option<String>,
}

/// What the screen draws.
#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
  pub tab: Tab,
  pub tabs: Vec<Tab>,
  pub save: Option<SaveButton>,
  pub application: Option<Application>,
  pub repository: Option<Repository>,
  pub preview: Option<Preview>,
  pub skills: Option<SkillsTab>,
  pub account: Option<AccountTab>,
  pub integrations: Option<IntegrationsTab>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SkillsTab {
  pub installed: usize,
  pub loading: bool,
  pub error: Option<String>,
  pub query: String,
  pub scope: String,
  pub scope_options: Vec<SelectOption>,
  pub rows: Vec<SkillRow>,
  /// The skill whose install dialog is open, by name.
  pub dialog: Option<SkillDialog>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SkillRow {
  pub id: String,
  pub name: String,
  pub source: String,
  pub description: Option<String>,
  pub has_url: bool,
  pub proprietary: bool,
  /// The install level, when installed.
  pub installed: Option<String>,
  pub busy: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SkillDialog {
  pub name: String,
  pub busy: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SaveButton {
  pub label: String,
  pub disabled: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Application {
  pub theme: String,
  pub theme_options: Vec<SelectOption>,
  pub zoom: f32,
  pub zoom_min: f32,
  pub zoom_max: f32,
  pub zoom_step: f32,
  pub font_size: String,
  pub font_size_options: Vec<SelectOption>,
  pub default_model: String,
  pub model_options: Vec<SelectOption>,
  pub default_agent: String,
  pub agent_options: Vec<SelectOption>,
  pub conflict_marker_style: String,
  pub conflict_options: Vec<SelectOption>,
  pub notify_agent_finished: bool,
  pub checklist: checklist::Checklist,
  pub setup_script: SetupScript,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SetupScript {
  pub script: String,
  pub always_run: bool,
  pub loading: bool,
  pub can_run: bool,
  pub status: String,
  pub status_error: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Repository {
  pub yaml_card: Option<YamlCard>,
  pub current_branch: Option<String>,
  pub loading: bool,
  pub fields: RepositoryFields,
  pub managed: Vec<String>,
  pub model_options: Vec<SelectOption>,
  pub agent_options: Vec<SelectOption>,
  pub review_agent_options: Vec<SelectOption>,
  pub trigger_options: Vec<SelectOption>,
  pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct YamlCard {
  pub error: Option<String>,
  pub loading: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RepositoryFields {
  pub branch_name_pattern: String,
  pub included_files: String,
  pub default_model: String,
  pub default_agent: String,
  pub auto_push: bool,
  pub review_prompt: String,
  pub review_agent: String,
  pub auto_review_trigger: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Preview {
  pub features: Vec<PreviewFeatureRow>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PreviewFeatureRow {
  pub id: String,
  pub title: String,
  pub checked: bool,
}

/// `{ value, label }` pairs as `SelectOption`s.
fn options(pairs: &[(&str, &str)]) -> Vec<SelectOption> {
  pairs
    .iter()
    .map(|(value, label)| SelectOption {
      value: value.to_string(),
      label: label.to_string(),
    })
    .collect()
}

/// The launchable agents (`core::agent_launch::AGENTS`), after a first
/// "default" option.
fn agent_options(default: &str) -> Vec<SelectOption> {
  options(&[
    ("", default),
    ("claude", "Claude"),
    ("codex", "Codex"),
    ("cursor", "Cursor"),
    ("copilot", "Copilot"),
  ])
}

/// `CLAUDE_MODEL_OPTIONS`, after a first "default" option.
fn model_options(default: &str) -> Vec<SelectOption> {
  options(&[
    ("", default),
    ("sonnet", "Sonnet"),
    ("opus", "Opus"),
    ("haiku", "Haiku"),
    ("sonnet[1m]", "Sonnet (1M)"),
    ("opusplan", "Opus Plan"),
  ])
}

/// The app setup script's status and unsaved edits.
#[derive(Default)]
struct Setup {
  status: Option<AppSetupScriptStatus>,
  error: Option<String>,
  script: Option<String>,
  always_run: Option<bool>,
  /// Set from a Run click until the status reports the run.
  starting: bool,
  polling: bool,
}

impl Setup {
  fn script(&self) -> String {
    self
      .script
      .clone()
      .or_else(|| self.status.as_ref().map(|s| s.script.clone()))
      .unwrap_or_default()
  }

  fn always_run(&self) -> bool {
    self
      .always_run
      .or_else(|| self.status.as_ref().map(|s| s.always_run))
      .unwrap_or(false)
  }

  /// `setupScriptRunState`.
  fn view(&self) -> SetupScript {
    let script = self.script();
    let unsaved = self.status.as_ref().is_some_and(|s| script != s.script);
    let can_run = self
      .status
      .as_ref()
      .is_some_and(|s| !s.running && !self.starting && !unsaved && !s.script.trim().is_empty());
    let (status, status_error) = match (&self.error, &self.status) {
      (Some(error), _) => (format!("Could not load the setup script: {error}"), true),
      (None, _) if unsaved => ("Save settings to run the edited script.".into(), false),
      (None, Some(status)) => (data::setup_status_text(status), false),
      (None, None) => (String::new(), false),
    };
    SetupScript {
      script,
      always_run: self.always_run(),
      loading: self.status.is_none(),
      can_run,
      status,
      status_error,
    }
  }
}

/// The Repository tab's state for one repository.
struct Repo {
  path: String,
  /// `None` until loaded.
  loaded: Option<Result<RepositoryFields, String>>,
  /// The fields as edited, kept across reloads, as React's draft.
  draft: Option<RepositoryFields>,
  config: Option<Result<RepoConfig, String>>,
  /// `.treq/config.yaml` is being read.
  config_loading: bool,
  saving: bool,
  save_error: Option<String>,
}

impl Repo {
  fn new(path: String) -> Self {
    Self {
      path,
      loaded: None,
      draft: None,
      config: None,
      config_loading: true,
      saving: false,
      save_error: None,
    }
  }

  fn fields(&self) -> RepositoryFields {
    match (&self.draft, &self.loaded) {
      (Some(draft), _) => draft.clone(),
      (None, Some(Ok(loaded))) => loaded.clone(),
      _ => RepositoryFields::default(),
    }
  }
}

/// The Skills tab (`SkillLibrarySettings`).
#[derive(Default)]
struct Skills {
  /// `None` while the registry loads.
  catalog: Option<Result<Vec<SkillCatalogSkillView>, String>>,
  installed: usize,
  query: String,
  /// The install-level filter: `all`, `application` or `repository`.
  scope: String,
  /// The skill an action runs for.
  pending: Option<String>,
  /// The skill whose install dialog is open.
  dialog: Option<String>,
}

/// The settings screen, its state from the backend, and its fields.
pub struct SettingsView {
  host: Arc<dyn SettingsHost>,
  /// React's handlers, for the tab and Close.
  react: ActionSink,
  /// This view's own handler, which the drawn controls call.
  sink: ActionSink,
  route: Route,
  dark: bool,
  controls: Controls,
  /// `None` until loaded.
  app: Option<AppFields>,
  preview: Vec<(PreviewFeature, bool)>,
  statuses: Option<Vec<PrerequisiteStatus>>,
  validating: bool,
  setup: Setup,
  repo: Option<Repo>,
  /// Loaded the first time the Skills tab shows.
  skills: Option<Skills>,
  account: account::State,
  tips: Tooltips,
}

impl HasTooltips for SettingsView {
  fn tooltips(&mut self) -> &mut Tooltips {
    &mut self.tips
  }
}

impl SettingsView {
  pub fn new(
    route: Route,
    host: Arc<dyn SettingsHost>,
    react: ActionSink,
    dark: bool,
    popups: PopupSink,
    cx: &mut Context<Self>,
  ) -> Self {
    let view = cx.weak_entity();
    // Controls report changes from inside this view's update, so handle them
    // after it.
    let sink: ActionSink = Rc::new(move |action, cx| {
      let view = view.clone();
      cx.defer(move |cx| {
        view
          .update(cx, |view, cx| view.handle(action, cx))
          .ignore_closed();
      });
    });
    let mut this = Self {
      host,
      react,
      controls: Controls::new(sink.clone()),
      sink,
      route: route.clone(),
      dark,
      app: None,
      preview: data::preview_flags(&data::Values::new()),
      statuses: None,
      validating: false,
      setup: Setup::default(),
      repo: None,
      skills: None,
      account: account::State::default(),
      tips: Tooltips::new(popups, dark),
    };
    this.load_app(cx);
    this.load_setup(cx);
    this.check(cx);
    this.set_repo(route.repo_path, cx);
    this.show_skills(cx);
    if this.route.tab == Tab::Integrations {
      this.load_linear(cx);
    }
    this.sync_account(cx);
    cx.observe_global::<super::account_data::Account>(|view, cx| {
      view.sync_account(cx);
      cx.notify();
    })
    .detach();
    this
  }

  pub fn set_route(&mut self, route: Route, dark: bool, cx: &mut Context<Self>) {
    if self.route != route || self.dark != dark {
      let repo_moved = self.route.repo_path != route.repo_path;
      let tab_moved = self.route.tab != route.tab;
      if repo_moved {
        self.set_repo(route.repo_path.clone(), cx);
        self.skills = None;
      }
      self.route = route;
      self.dark = dark;
      self.tips.dark = dark;
      self.show_skills(cx);
      if self.route.tab == Tab::Integrations && (repo_moved || tab_moved) {
        self.load_linear(cx);
      }
      self.sync_account(cx);
      cx.notify();
    }
  }

  /// Runs `work` off the main thread, then `done` with its result here.
  fn spawn<T: Send + 'static>(
    &self,
    cx: &mut Context<Self>,
    work: impl FnOnce(&dyn SettingsHost) -> T + Send + 'static,
    done: impl FnOnce(&mut Self, T, &mut Context<Self>) + 'static,
  ) {
    let host = self.host.clone();
    cx.spawn(async move |this, cx| {
      let result = cx.background_spawn(async move { work(&*host) }).await;
      this
        .update(cx, |view, cx| {
          done(view, result, cx);
          cx.notify();
        })
        .ignore_closed();
    })
    .detach();
  }

  fn load_app(&mut self, cx: &mut Context<Self>) {
    self.spawn(
      cx,
      |host| host.settings(&data::app_keys()),
      |view, values, _| match values {
        Ok(values) => {
          view.app = Some(AppFields::from_values(&values));
          view.preview = data::preview_flags(&values);
        }
        Err(err) => log::warn!("failed to load the settings: {err}"),
      },
    );
  }

  fn load_setup(&mut self, cx: &mut Context<Self>) {
    self.spawn(
      cx,
      |host| host.setup_script(),
      |view, status, cx| view.set_setup_status(status, cx),
    );
  }

  fn set_setup_status(
    &mut self,
    status: Result<AppSetupScriptStatus, String>,
    cx: &mut Context<Self>,
  ) {
    match status {
      Ok(status) => {
        let running = status.running;
        self.setup.status = Some(status);
        self.setup.error = None;
        if running {
          self.poll_setup(cx);
        }
      }
      Err(err) => self.setup.error = Some(err),
    }
  }

  /// Reads the status every second while the script runs.
  fn poll_setup(&mut self, cx: &mut Context<Self>) {
    if self.setup.polling {
      return;
    }
    self.setup.polling = true;
    let host = self.host.clone();
    cx.spawn(async move |this, cx| loop {
      cx.background_executor().timer(Duration::from_secs(1)).await;
      let host = host.clone();
      let status = cx
        .background_spawn(async move { host.setup_script() })
        .await;
      let running = status.as_ref().is_ok_and(|s| s.running);
      let alive = this
        .update(cx, |view, cx| {
          if let Ok(status) = status {
            view.setup.status = Some(status);
          }
          view.setup.polling = running;
          cx.notify();
        })
        .is_ok();
      if !running || !alive {
        break;
      }
    })
    .detach();
  }

  fn check(&mut self, cx: &mut Context<Self>) {
    self.validating = true;
    cx.notify();
    self.spawn(
      cx,
      |host| host.check_prerequisites(),
      |view, statuses, _| {
        view.statuses = Some(statuses);
        view.validating = false;
      },
    );
  }

  fn set_repo(&mut self, path: Option<String>, cx: &mut Context<Self>) {
    self.repo = path.filter(|p| !p.is_empty()).map(Repo::new);
    self.load_repo(cx);
  }

  /// Reads `.treq/config.yaml`, which writes what it sets into the
  /// repository's settings, then the settings.
  fn load_repo(&mut self, cx: &mut Context<Self>) {
    let Some(repo) = &mut self.repo else {
      return;
    };
    repo.config_loading = true;
    let path = repo.path.clone();
    self.spawn(
      cx,
      {
        let path = path.clone();
        move |host| {
          let config = host.repo_config(&path);
          let fields = host
            .repo_settings(&path, &data::repo_keys())
            .map(|values| RepositoryFields::from_values(&values));
          (config, fields)
        }
      },
      move |view, (config, fields), _| {
        let Some(repo) = view.repo.as_mut().filter(|repo| repo.path == path) else {
          return;
        };
        repo.config = Some(config);
        repo.config_loading = false;
        repo.loaded = Some(fields);
      },
    );
  }

  /// Writes app settings now, as the theme, zoom and preview switches do.
  /// Loads the skill registry the first time the Skills tab shows.
  fn show_skills(&mut self, cx: &mut Context<Self>) {
    if self.route.tab == Tab::Skills && self.skills.is_none() {
      self.skills = Some(Skills {
        scope: "all".into(),
        ..Skills::default()
      });
      self.load_skills(cx);
    }
  }

  fn load_skills(&mut self, cx: &mut Context<Self>) {
    let repo = self.route.repo_path.clone();
    self.spawn(
      cx,
      move |host| {
        let repo = repo.as_deref();
        (host.skill_catalog(repo), host.installed_skills(repo))
      },
      |view, (catalog, installed), _| {
        if let Some(skills) = &mut view.skills {
          skills.catalog = Some(catalog);
          skills.installed = installed.map_or(0, |list| list.len());
        }
      },
    );
  }

  /// Runs a skill action, then reads the registry again, as `runAction`.
  fn skill_action(
    &mut self,
    skill_id: String,
    cx: &mut Context<Self>,
    work: impl FnOnce(&dyn SettingsHost, Option<&str>) -> Result<(), String> + Send + 'static,
  ) {
    let Some(skills) = &mut self.skills else {
      return;
    };
    if skills.pending.is_some() {
      return;
    }
    skills.pending = Some(skill_id);
    let repo = self.route.repo_path.clone();
    self.spawn(
      cx,
      move |host| work(host, repo.as_deref()),
      |view, result, cx| {
        if let Err(err) = result {
          view
            .host
            .notify(toast(ToastTone::Error, "Skill library", err));
        }
        if let Some(skills) = &mut view.skills {
          skills.pending = None;
        }
        view.load_skills(cx);
      },
    );
  }

  fn press_skill(&mut self, action: &str, id: &str, cx: &mut Context<Self>) {
    let id = id.to_string();
    match action {
      "skills.install" => {
        if let Some(skills) = &mut self.skills {
          skills.dialog = Some(id);
        }
      }
      "skills.dialog" => {
        let Some(skill_id) = self.skills.as_mut().and_then(|s| s.dialog.take()) else {
          return;
        };
        if let Some(scope) = data::parse_scope(&id) {
          self.skill_action(skill_id.clone(), cx, move |host, repo| {
            host.install_skill(&skill_id, scope, repo)
          });
        }
      }
      "skills.uninstall" => self.skill_action(id.clone(), cx, move |host, repo| {
        host.uninstall_skill(&id, repo)
      }),
      "skills.source" => {
        let url = self
          .skills
          .as_ref()
          .and_then(|s| s.catalog.as_ref()?.as_ref().ok())
          .and_then(|list| list.iter().find(|v| v.skill.id == id))
          .and_then(|v| v.skill.url.clone());
        if let Some(url) = url {
          self.host.open_url(&url);
        }
      }
      _ => {}
    }
  }

  fn write_settings(&self, values: Vec<(String, String)>, cx: &mut Context<Self>) {
    self.spawn(
      cx,
      move |host| {
        let result = host.set_settings(&values);
        if result.is_ok() {
          host.notify(NativeEvent::SettingsChanged);
        }
        result
      },
      |_, result, _| {
        if let Err(err) = result {
          log::warn!("failed to save a setting: {err}");
        }
      },
    );
  }

  fn save_application(&mut self, cx: &mut Context<Self>) {
    let Some(app) = self.app.clone() else {
      return;
    };
    // Unedited, the script may be the "" of an unloaded status.
    let script = (self.setup.script.is_some() || self.setup.always_run.is_some())
      .then(|| (self.setup.script(), self.setup.always_run()));
    self.spawn(
      cx,
      move |host| {
        let result = host
          .set_settings(&app.to_values())
          .and_then(|()| match &script {
            Some((script, always_run)) => host.save_setup_script(script, *always_run),
            None => Ok(()),
          });
        match &result {
          Ok(()) => {
            host.notify(NativeEvent::SettingsChanged);
            host.notify(toast(ToastTone::Success, "Settings saved", ""));
          }
          Err(err) => host.notify(toast(ToastTone::Error, "Error", err)),
        }
        (result, host.setup_script())
      },
      |view, (_, status), cx| view.set_setup_status(status, cx),
    );
  }

  fn save_repository(&mut self, cx: &mut Context<Self>) {
    let Some(repo) = self.repo.as_mut().filter(|repo| !repo.saving) else {
      return;
    };
    repo.saving = true;
    let (path, values) = (repo.path.clone(), repo.fields().to_values());
    self.spawn(
      cx,
      {
        let path = path.clone();
        move |host| {
          let result = host.set_repo_settings(&path, &values);
          match &result {
            Ok(()) => {
              host.notify(NativeEvent::RepoSettingsChanged { repo_path: path });
              host.notify(toast(ToastTone::Success, "Settings saved", ""));
            }
            Err(err) => host.notify(toast(
              ToastTone::Error,
              "Error",
              format!("Failed to save settings: {err}"),
            )),
          }
          result
        }
      },
      move |view, result, _| {
        let Some(repo) = view.repo.as_mut().filter(|repo| repo.path == path) else {
          return;
        };
        repo.saving = false;
        if let Err(err) = result {
          repo.save_error = Some(format!("Failed to save settings: {err}"));
        }
      },
    );
  }

  fn run_setup(&mut self, cx: &mut Context<Self>) {
    if !self.setup.view().can_run {
      return;
    }
    self.setup.starting = true;
    self.spawn(
      cx,
      |host| {
        host.run_setup_script();
        host.setup_script()
      },
      |view, status, cx| {
        view.setup.starting = false;
        view.set_setup_status(status, cx);
      },
    );
  }

  pub(super) fn handle(&mut self, action: ScreenAction, cx: &mut Context<Self>) {
    match action {
      ScreenAction::Press { id } if self.press_account(&id, cx) => {}
      ScreenAction::Press { id } => match id.split_once(':') {
        Some((action, skill)) if action.starts_with("skills.") => {
          let skill = skill.to_string();
          self.press_skill(action, &skill, cx);
        }
        Some(("preview.docs", feature)) => {
          if let Some((_, _, path)) = data::PREVIEW_FEATURES
            .iter()
            .find(|(f, _, _)| f.as_str() == feature)
          {
            self.host.open_url(&format!("{}{path}", native::web_url()));
          }
        }
        _ => match id.as_str() {
          "settings.save" => match self.route.tab {
            Tab::Application => self.save_application(cx),
            Tab::Repository => self.save_repository(cx),
            _ => {}
          },
          "app.setupScript.run" => self.run_setup(cx),
          "repo.yaml.reload" => self.load_repo(cx),
          "skills.dialog.cancel" => {
            if let Some(skills) = &mut self.skills {
              skills.dialog = None;
            }
          }
          _ => (self.react)(ScreenAction::Press { id }, cx),
        },
      },
      ScreenAction::Field { field, value } => self.set_field(field, value, cx),
      ScreenAction::Commit { field, value } if field == "app.zoom" => {
        let zoom = value.as_f64().unwrap_or(100.) as f32;
        if let Some(app) = &mut self.app {
          app.zoom = zoom;
        }
        self.write_settings(
          vec![("ui_zoom".into(), (zoom.round() as i64).to_string())],
          cx,
        );
      }
      ScreenAction::RecheckPrerequisites => self.check(cx),
      ScreenAction::OpenUrl { url } => self.host.open_url(&url),
      other => (self.react)(other, cx),
    }
    cx.notify();
  }

  fn set_field(&mut self, field: String, value: serde_json::Value, cx: &mut Context<Self>) {
    if self.set_account_field(&field, &value) {
      return;
    }
    let text = value.as_str().unwrap_or_default().to_string();
    if let Some(name) = field.strip_prefix("repo.") {
      if let Some(repo) = &mut self.repo {
        let mut fields = repo.fields();
        fields.set(name, &value);
        repo.draft = Some(fields);
      }
      return;
    }
    if let Some(name) = field.strip_prefix("skills.") {
      if let Some(id) = name.strip_prefix("level:") {
        if let Some(scope) = data::parse_scope(&text) {
          let id = id.to_string();
          self.skill_action(id.clone(), cx, move |host, repo| {
            host.set_skill_scope(&id, scope, repo)
          });
        }
      } else if let Some(skills) = &mut self.skills {
        match name {
          "query" => skills.query = text,
          "scope" => skills.scope = text,
          _ => {}
        }
      }
      return;
    }
    if let Some(id) = field.strip_prefix("preview.") {
      let on = value.as_bool().unwrap_or_default();
      if let Some((feature, checked)) = self.preview.iter_mut().find(|(f, _)| f.as_str() == id) {
        *checked = on;
        let key = feature.setting_key();
        self.write_settings(vec![(key, on.to_string())], cx);
      }
      return;
    }
    match field.as_str() {
      "app.setupScript" => self.setup.script = Some(text),
      "app.setupScriptAlwaysRun" => self.setup.always_run = value.as_bool(),
      "settings.tab" => (self.react)(ScreenAction::Field { field, value }, cx),
      _ => {
        let Some(app) = &mut self.app else {
          return;
        };
        match field.as_str() {
          "app.theme" => {
            app.theme = text.clone();
            self.write_settings(vec![("theme".into(), text)], cx);
          }
          "app.zoom" => app.zoom = value.as_f64().unwrap_or(100.) as f32,
          "app.fontSize" => {
            if let Ok(size) = text.parse() {
              app.font_size = size;
            }
          }
          "app.defaultModel" => app.default_model = text,
          "app.defaultAgent" => app.default_agent = text,
          "app.conflictMarkerStyle" => app.conflict_marker_style = text,
          "app.notifyAgentFinished" => app.notify_agent_finished = value.as_bool().unwrap_or(true),
          _ => {}
        }
      }
    }
  }

  fn preview_on(&self, feature: PreviewFeature) -> bool {
    self.preview.iter().any(|(f, on)| *f == feature && *on)
  }

  /// What to draw, from the state above.
  pub(super) fn screen(&self) -> Settings {
    let tab = self.route.tab;
    let mut tabs = vec![
      Tab::Repository,
      Tab::Application,
      Tab::Account,
      Tab::Integrations,
    ];
    if self.preview_on(PreviewFeature::SkillsInstallation) {
      tabs.push(Tab::Skills);
    }
    tabs.push(Tab::Preview);
    let save = match tab {
      Tab::Application => Some(SaveButton {
        label: "Save Settings".into(),
        disabled: false,
      }),
      Tab::Repository => self.repo.as_ref().map(|repo| SaveButton {
        label: if repo.saving {
          "Saving..."
        } else {
          "Save Settings"
        }
        .into(),
        disabled: repo.saving,
      }),
      _ => None,
    };
    let application = (tab == Tab::Application).then(|| {
      let app = self
        .app
        .clone()
        .unwrap_or_else(|| AppFields::from_values(&data::Values::new()));
      Application {
        theme: app.theme,
        theme_options: options(&[("system", "System"), ("light", "Light"), ("dark", "Dark")]),
        zoom: app.zoom,
        zoom_min: data::ZOOM.0,
        zoom_max: data::ZOOM.1,
        zoom_step: data::ZOOM.2,
        font_size: data::nearest_font_size(app.font_size).to_string(),
        font_size_options: data::FONT_SIZES
          .iter()
          .map(|(size, label)| SelectOption {
            value: size.to_string(),
            label: label.to_string(),
          })
          .collect(),
        default_model: app.default_model,
        model_options: model_options("Default"),
        default_agent: app.default_agent,
        agent_options: agent_options("Default (Claude)"),
        conflict_marker_style: app.conflict_marker_style,
        conflict_options: options(&[
          ("git", "Git Diff3 (Default)"),
          ("diff", "JJ Diff"),
          ("snapshot", "JJ Snapshot"),
        ]),
        notify_agent_finished: app.notify_agent_finished,
        checklist: checklist::from_statuses(
          "Command-line tools",
          self.statuses.as_deref(),
          self.validating,
        ),
        setup_script: self.setup.view(),
      }
    });
    let repository = self
      .repo
      .as_ref()
      .filter(|_| tab == Tab::Repository)
      .map(|repo| {
        let config = repo.config.as_ref().and_then(|c| c.as_ref().ok());
        let config_error = repo
          .config
          .as_ref()
          .and_then(|c| c.as_ref().err())
          .map(|err| format!("Failed to load .treq/config.yaml: {err}"));
        let load_error = repo
          .loaded
          .as_ref()
          .and_then(|l| l.as_ref().err())
          .map(|err| format!("Failed to load settings: {err}"));
        Repository {
          yaml_card: (config_error.is_some() || config.is_some_and(data::sets_any_value))
            .then_some(YamlCard {
              error: config_error,
              loading: repo.config_loading,
            }),
          current_branch: self.route.current_branch.clone(),
          loading: repo.loaded.is_none() || repo.config_loading,
          fields: repo.fields(),
          managed: config.map(data::managed_fields).unwrap_or_default(),
          model_options: model_options("Use Application Default"),
          agent_options: agent_options("Use Application Default"),
          review_agent_options: agent_options("Use Default Agent"),
          trigger_options: options(&[
            ("off", "Off"),
            ("on-commit", "On commit"),
            ("on-rebase", "On rebase"),
            ("on-pull", "On pull from remote"),
          ]),
          error: repo.save_error.clone().or(load_error),
        }
      });
    let preview = (tab == Tab::Preview).then(|| Preview {
      features: data::PREVIEW_FEATURES
        .iter()
        .map(|(feature, title, _)| PreviewFeatureRow {
          id: feature.as_str().to_string(),
          title: title.to_string(),
          checked: self.preview_on(*feature),
        })
        .collect(),
    });
    let skills = (tab == Tab::Skills)
      .then_some(self.skills.as_ref())
      .flatten()
      .map(|skills| {
        let pending = skills.pending.as_deref();
        let (loading, error, catalog) = match &skills.catalog {
          None => (true, None, &[][..]),
          Some(Err(err)) => (false, Some(err.clone()), &[][..]),
          Some(Ok(list)) => (false, None, &list[..]),
        };
        let rows = data::filter_skills(catalog, &skills.query, &skills.scope)
          .into_iter()
          .map(|view| SkillRow {
            id: view.skill.id.clone(),
            name: view.skill.name.clone(),
            source: view.skill.source.clone(),
            description: view.skill.description.clone().filter(|d| !d.is_empty()),
            has_url: view.skill.url.is_some(),
            proprietary: view.skill.proprietary,
            installed: view
              .installed
              .as_ref()
              .map(|i| data::scope_value(i.scope).to_string()),
            busy: pending == Some(view.skill.id.as_str()),
          })
          .collect();
        SkillsTab {
          installed: skills.installed,
          loading,
          error,
          query: skills.query.clone(),
          scope: skills.scope.clone(),
          scope_options: options(&[
            ("all", "All"),
            ("application", "Application"),
            ("repository", "Repository"),
          ]),
          rows,
          dialog: skills.dialog.as_ref().map(|id| SkillDialog {
            name: catalog
              .iter()
              .find(|v| &v.skill.id == id)
              .map_or_else(|| id.clone(), |v| v.skill.name.clone()),
            busy: pending == Some(id.as_str()),
          }),
        }
      });
    Settings {
      tab,
      tabs,
      save,
      application,
      repository,
      preview,
      skills,
      account: (tab == Tab::Account).then(|| self.account_tab()),
      integrations: (tab == Tab::Integrations).then(|| self.integrations_tab()),
    }
  }

  fn press(&self, id: &str) -> impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static {
    let (sink, id) = (self.sink.clone(), id.to_string());
    move |_, _, cx| sink(ScreenAction::Press { id: id.clone() }, cx)
  }

  fn field(&self, field: &str) -> impl Fn(bool, &mut Window, &mut gpui::App) + 'static {
    let (sink, field) = (self.sink.clone(), field.to_string());
    move |value, _, cx| {
      sink(
        ScreenAction::Field {
          field: field.clone(),
          value: value.into(),
        },
        cx,
      )
    }
  }
}

fn toast(tone: ToastTone, title: &str, description: impl ToString) -> NativeEvent {
  NativeEvent::Toast {
    title: title.into(),
    description: description.to_string(),
    tone,
    action: None,
  }
}

/// `<Label>`: 13px medium, `leading-none`.
fn label(text: impl Into<SharedString>) -> gpui::Div {
  ui::text(div(), TextSize::Sm)
    .h(zpx(13.))
    .font_weight(FontWeight::MEDIUM)
    .child(text.into())
}

/// The muted help line under a field, `mt-1` or `mt-2`.
fn help(text: impl Into<SharedString>, p: &Palette, mt: f32) -> gpui::Div {
  ui::text(div().mt(zpx(mt)), TextSize::Sm)
    .text_color(p.muted_foreground)
    .child(text.into())
}

/// The help line under Branch Name Pattern: the pattern and the branch it
/// gives a sample workspace name, following unsaved edits.
fn branch_preview(pattern: &str) -> String {
  let pattern = match pattern.trim() {
    "" => "treq/{name}",
    pattern => pattern,
  };
  format!(
    "{pattern} → {}",
    super::code_data::apply_branch_name_pattern(pattern, "add dark mode")
  )
}

/// A label above a control, with a help line under it.
fn field_block(
  name: &str,
  control: impl IntoElement,
  help_text: Option<&str>,
  p: &Palette,
) -> gpui::Div {
  div()
    .child(label(name.to_string()))
    .child(div().mt_2().child(control))
    .when_some(help_text, |this, text| {
      this.child(help(text.to_string(), p, 4.))
    })
}

/// A label and help text on the left, a switch on the right.
fn switch_row(name: &str, help_text: &str, switch: impl IntoElement, p: &Palette) -> gpui::Div {
  div()
    .flex()
    .items_center()
    .justify_between()
    .gap_4()
    .child(
      div()
        .child(label(name.to_string()))
        .child(help(help_text.to_string(), p, 4.)),
    )
    .child(switch)
}

impl Render for SettingsView {
  fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    self.tips.sync(cx);
    let p = theme::palette(self.dark);
    let s = self.screen();

    let header = div()
      .flex()
      .items_center()
      .justify_between()
      .border_b_1()
      .border_color(p.border)
      .px_6()
      .py_2()
      .child(
        ui::text(div(), TextSize::Lg)
          .font_weight(FontWeight::SEMIBOLD)
          .child("Settings"),
      )
      .child(
        div()
          .flex()
          .items_center()
          .gap_2()
          .child(
            Button::new("settings-close", &p)
              .variant(Variant::Outline)
              .size(Size::Sm)
              .label("Close")
              .on_click(self.press("settings.close")),
          )
          .when_some(s.save.clone(), |this, save| {
            this.child(
              Button::new("settings-save", &p)
                .size(Size::Sm)
                .disabled(save.disabled)
                .label(save.label)
                .on_click(self.press("settings.save")),
            )
          }),
      );

    let tabs = div()
      .w(zpx(192.))
      .flex_none()
      .flex()
      .flex_col()
      .gap_1()
      .children(s.tabs.iter().map(|&tab| {
        let active = tab == s.tab;
        let id = SharedString::from(format!("settings-tab-{}", tab.value()));
        let selector = id.to_string();
        let sink = self.sink.clone();
        ui::text(div(), TextSize::Sm)
          .id(id)
          .debug_selector(move || selector)
          .role(Role::Tab)
          .aria_label(tab.label())
          .aria_selected(active)
          .test_support()
          .flex()
          .items_center()
          .gap_2()
          .px_4()
          .py_2()
          .rounded(radius::MD)
          .font_weight(FontWeight::MEDIUM)
          .cursor_pointer()
          .map(|this| {
            if active {
              this.bg(p.accent).text_color(p.foreground)
            } else {
              this
                .text_color(p.muted_foreground)
                .hover(|style| style.bg(p.accent.opacity(0.5)).text_color(p.foreground))
            }
          })
          .child(ui::icon(
            tab.icon(),
            16.,
            if active {
              p.foreground
            } else {
              p.muted_foreground
            },
          ))
          .child(tab.label())
          .on_click(move |_, _, cx| {
            sink(
              ScreenAction::Field {
                field: "settings.tab".into(),
                value: tab.value().into(),
              },
              cx,
            )
          })
      }));

    let content: AnyElement = match s.tab {
      Tab::Application => match &s.application {
        Some(app) => self.application(app, &p, window, cx).into_any_element(),
        None => div().into_any_element(),
      },
      Tab::Repository => match &s.repository {
        Some(repo) => self.repository(repo, &p, window, cx).into_any_element(),
        None => div()
          .text_center()
          .py_8()
          .text_color(p.muted_foreground)
          .child("Set repository in Application tab")
          .into_any_element(),
      },
      Tab::Preview => match &s.preview {
        Some(preview) => self.preview(preview, &p).into_any_element(),
        None => div().into_any_element(),
      },
      Tab::Skills => match &s.skills {
        Some(skills) => self.skills(skills, &p, window, cx).into_any_element(),
        None => div().into_any_element(),
      },
      Tab::Account => match &s.account {
        Some(account) => self.account(account, &p, window, cx),
        None => div().into_any_element(),
      },
      Tab::Integrations => match &s.integrations {
        Some(integrations) => self
          .integrations(integrations, &p, window, cx)
          .into_any_element(),
        None => div().into_any_element(),
      },
    };
    let dialog = s
      .skills
      .as_ref()
      .and_then(|skills| skills.dialog.clone())
      .map(|dialog| self.skill_dialog(&dialog, &p));
    let delete_dialog = match &s.account {
      Some(AccountTab::SignedIn(signed_in))
        if signed_in
          .cloud
          .as_ref()
          .is_some_and(|c| c.confirming_delete) =>
      {
        Some(self.cloud_delete_dialog(&p).into_any_element())
      }
      _ => None,
    };

    div()
      .relative()
      .size_full()
      .flex()
      .flex_col()
      .bg(p.background)
      .child(header)
      .child(
        div()
          .id("settings-scroll")
          .debug_selector(|| "settings-scroll".into())
          .flex_1()
          .min_h_0()
          .overflow_y_scroll()
          .child(
            div()
              .max_w(zpx(896.))
              .mx_auto()
              .p_6()
              .flex()
              .gap_8()
              .child(tabs)
              .child(div().flex_1().min_w_0().child(content)),
          ),
      )
      .children(dialog)
      .children(delete_dialog)
  }
}

impl SettingsView {
  fn application(
    &mut self,
    app: &Application,
    p: &Palette,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> impl IntoElement {
    let theme_select =
      self
        .controls
        .select("app.theme", &app.theme_options, &app.theme, window, cx);
    let zoom = self.controls.slider(
      "app.zoom",
      app.zoom_min,
      app.zoom_max,
      app.zoom_step,
      app.zoom,
      window,
      cx,
    );
    let font = self.controls.select(
      "app.fontSize",
      &app.font_size_options,
      &app.font_size,
      window,
      cx,
    );
    let model = self.controls.select(
      "app.defaultModel",
      &app.model_options,
      &app.default_model,
      window,
      cx,
    );
    let agent = self.controls.select(
      "app.defaultAgent",
      &app.agent_options,
      &app.default_agent,
      window,
      cx,
    );
    let conflict = self.controls.select(
      "app.conflictMarkerStyle",
      &app.conflict_options,
      &app.conflict_marker_style,
      window,
      cx,
    );
    let script = self.controls.textarea(
      "app.setupScript",
      &app.setup_script.script,
      "brew install jj",
      4,
      window,
      cx,
    );
    let ctx = super::Ctx {
      palette: *p,
      sink: &self.sink,
    };
    let setup = &app.setup_script;

    div()
      .flex()
      .flex_col()
      .gap_6()
      .child(field_block(
        "Theme",
        form::select_field("settings-theme", "Theme", &theme_select, false),
        None,
        p,
      ))
      .child(
        div()
          .child(
            div()
              .flex()
              .items_center()
              .justify_between()
              .child(label("UI Zoom"))
              .child(
                ui::text(div(), TextSize::Sm)
                  .text_color(p.muted_foreground)
                  .child(format!("{}%", app.zoom)),
              ),
          )
          .child(div().mt_3().child(form::slider_field(&zoom)))
          .child(help(
            if cfg!(target_os = "macos") {
              "Scale the application interface. Use ⌘ + + to zoom in and ⌘ + - to zoom out in 5% steps."
            } else {
              "Scale the application interface. Use Ctrl + + to zoom in and Ctrl + - to zoom out in 5% steps."
            },
            p,
            8.,
          )),
      )
      .child(
        div()
          .child(label("Font Size"))
          .child(
            div()
              .mt_2()
              .child(form::select_field("settings-font-size", "Font Size", &font, false)),
          )
          .child(help(
            "Font size for terminal, code diff display, and overview files",
            p,
            8.,
          )),
      )
      .child(field_block(
        "Claude Code Model",
        form::select_field("settings-default-model", "Claude Code Model", &model, false),
        Some("Default model for new Claude Code sessions"),
        p,
      ))
      .child(field_block(
        "Default Agent",
        form::select_field("settings-default-agent", "Default Agent", &agent, false),
        Some("Default agent for new sessions"),
        p,
      ))
      .child(checklist::render(&app.checklist, &ctx))
      .child(field_block(
        "Conflict Marker Style",
        form::select_field("settings-conflict-marker-style", "Conflict Marker Style", &conflict, false),
        Some("Style of conflict markers written to files during rebase. Git Diff3 is compatible with most editors."),
        p,
      ))
      .child(
        div()
          .flex()
          .items_start()
          .justify_between()
          .gap_4()
          .child(
            div()
              .child(label("Notify when an agent finishes"))
              .child(help(
                "Show an OS notification when an agent finishes or asks for input while no Treq window is focused.",
                p,
                4.,
              )),
          )
          .child(div().mt_1().child(form::switch(
            "settings-notify-agent-finished",
            "Notify when an agent finishes",
            app.notify_agent_finished,
            false,
            p,
            self.field("app.notifyAgentFinished"),
          ))),
      )
      .child(
        div()
          .child(label("Application Setup Script"))
          .child(
            div()
              .mt_2()
              .child(form::textarea_field(&script, true, setup.loading)),
          )
          .child(help(
            "Shell script run from your home directory when you click Run. Its output is in the Logs tab under App setup.",
            p,
            4.,
          ))
          .child(
            div()
              .flex()
              .items_center()
              .gap_2()
              .mt_3()
              .child(form::switch(
                "settings-setup-always-run",
                "Run on every app startup",
                setup.always_run,
                setup.loading,
                p,
                self.field("app.setupScriptAlwaysRun"),
              ))
              .child(label("Run on every app startup")),
          )
          .child(
            div()
              .flex()
              .items_center()
              .gap_3()
              .mt_3()
              .child(
                Button::new("settings-setup-run", p)
                  .variant(Variant::Outline)
                  .size(Size::Sm)
                  .disabled(!setup.can_run)
                  .child(
                    div()
                      .flex()
                      .mr_1()
                      .child(ui::icon(theme::ICON_PLAY, 12., p.foreground)),
                  )
                  .label("Run")
                  .on_click(self.press("app.setupScript.run")),
              )
              .child(
                ui::text(div(), TextSize::Sm)
                  .text_color(if setup.status_error {
                    p.destructive
                  } else {
                    p.muted_foreground
                  })
                  .child(setup.status.clone()),
              ),
          ),
      )
  }

  fn repository(
    &mut self,
    repo: &Repository,
    p: &Palette,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> impl IntoElement {
    let managed = |field: &str| repo.managed.iter().any(|m| m == field);
    let f = &repo.fields;

    let yaml_card = repo.yaml_card.as_ref().map(|card| {
      let reload = Button::new("settings-yaml-reload", p)
        .variant(Variant::Outline)
        .size(Size::Sm)
        .disabled(card.loading)
        .label(if card.loading {
          "Reloading..."
        } else {
          "Reload"
        })
        .on_click(self.press("repo.yaml.reload"));
      let body = match &card.error {
        Some(error) => ui::text(div(), TextSize::Sm)
          .text_color(p.destructive)
          .child(error.clone()),
        None => ui::text(div(), TextSize::Sm)
          .child(
            div()
              .font_weight(FontWeight::MEDIUM)
              .child("Synced from .treq/config.yaml."),
          )
          .child(div().text_color(p.muted_foreground).child(
            "Settings shown as disabled below are managed by that file — edit it directly to change them.",
          )),
      };
      ui::card(p)
        .when(card.error.is_some(), |this| {
          this.border_color(p.destructive.opacity(0.5))
        })
        .when(card.error.is_none(), |this| this.bg(p.muted.opacity(0.5)))
        .flex()
        .items_center()
        .justify_between()
        .gap_4()
        .p_4()
        .child(div().flex_1().child(body))
        .child(reload)
    });

    let branch = repo.current_branch.clone().map(|branch| {
      div()
        .border_b_1()
        .border_color(p.border)
        .pb_6()
        .child(
          ui::text(div(), TextSize::Sm)
            .text_color(p.muted_foreground)
            .child("Current Branch"),
        )
        .child(
          ui::text(div(), TextSize::Sm)
            .mt_1()
            .flex()
            .items_center()
            .gap_2()
            .child(ui::icon(theme::ICON_BRANCH, 12., p.foreground))
            .child(div().font_family(theme::MONO_FONT).child(branch)),
        )
    });

    let body: AnyElement = if repo.loading {
      ui::delayed(
        "settings-loading",
        div()
          .py_8()
          .text_center()
          .text_color(p.muted_foreground)
          .child("Loading settings..."),
      )
      .into_any_element()
    } else {
      let pattern = self.controls.input(
        "repo.branchNamePattern",
        &f.branch_name_pattern,
        "treq/{name}",
        window,
        cx,
      );
      let included = self.controls.textarea(
        "repo.includedFiles",
        &f.included_files,
        "e.g., .env\n.env.local",
        4,
        window,
        cx,
      );
      let model = self.controls.select(
        "repo.defaultModel",
        &repo.model_options,
        &f.default_model,
        window,
        cx,
      );
      let agent = self.controls.select(
        "repo.defaultAgent",
        &repo.agent_options,
        &f.default_agent,
        window,
        cx,
      );
      let review_prompt = self.controls.textarea(
        "repo.reviewPrompt",
        &f.review_prompt,
        "Leave empty to use the built-in review prompt",
        6,
        window,
        cx,
      );
      let review_agent = self.controls.select(
        "repo.reviewAgent",
        &repo.review_agent_options,
        &f.review_agent,
        window,
        cx,
      );
      let trigger = self.controls.select(
        "repo.autoReviewTrigger",
        &repo.trigger_options,
        &f.auto_review_trigger,
        window,
        cx,
      );
      div()
        .flex()
        .flex_col()
        .gap_6()
        .child(field_block(
          "Branch Name Pattern",
          form::input_field("settings-branch-pattern", &pattern, true, managed("branchNamePattern")),
          Some(&branch_preview(&f.branch_name_pattern)),
          p,
        ))
        .child(field_block(
          "Included Files/Directories",
          form::textarea_field(&included, true, managed("includedFiles")),
          Some("Paths to copy into each new workspace (e.g. .env). For heavy dirs like node_modules, use Symlink from home repo under Advanced when creating a workspace."),
          p,
        ))
        .child(field_block(
          "Claude Code Model",
          form::select_field("settings-repo-default-model", "Claude Code Model", &model, managed("defaultModel")),
          Some("Default model for new Claude Code sessions in this repository (overrides application default)"),
          p,
        ))
        .child(field_block(
          "Default Agent",
          form::select_field("settings-repo-default-agent", "Default Agent", &agent, managed("defaultAgent")),
          Some("Default agent for new sessions in this repository (overrides application default)"),
          p,
        ))
        .child(switch_row(
          "Auto-push to remote",
          "Automatically push to the remote after every commit in this repository",
          form::switch(
            "settings-auto-push",
            "Auto-push to remote",
            f.auto_push,
            false,
            p,
            self.field("repo.autoPush"),
          ),
          p,
        ))
        .child(
          div()
            .flex()
            .flex_col()
            .gap_4()
            .border_t_1()
            .border_color(p.border)
            .pt_6()
            .child(
              div()
                .child(
                  ui::text(div(), TextSize::Sm)
                    .font_weight(FontWeight::MEDIUM)
                    .child("Code Review"),
                )
                .child(help(
                  "Settings for the AI review agent. Its comments stay local to this machine and are never pushed to GitHub.",
                  p,
                  4.,
                )),
            )
            .child(field_block(
              "Custom Review Prompt",
              form::textarea_field(&review_prompt, true, managed("reviewPrompt")),
              Some("Overrides the built-in prompt. {target_type}, {target_id} and {diff_summary} are substituted before the prompt is sent."),
              p,
            ))
            .child(field_block(
              "Review Agent",
              form::select_field("settings-review-agent", "Review Agent", &review_agent, managed("reviewAgent")),
              Some("Agent launched by Start Review (overrides the default agent)"),
              p,
            ))
            .child(field_block(
              "Automatic Review",
              form::select_field("settings-auto-review-trigger", "Automatic Review", &trigger, managed("autoReviewTrigger")),
              Some("Opens a review terminal on its own after the chosen operation finishes in a workspace of this repository."),
              p,
            )),
        )
        .when_some(repo.error.clone(), |this, error| {
          this.child(
            ui::text(div(), TextSize::Sm)
              .text_color(p.destructive)
              .child(error),
          )
        })
        .into_any_element()
    };

    div()
      .flex()
      .flex_col()
      .gap_6()
      .children(yaml_card)
      .children(branch)
      .child(body)
  }

  fn preview(&self, preview: &Preview, p: &Palette) -> impl IntoElement {
    div()
      .flex()
      .flex_col()
      .gap_6()
      .child(
        div()
          .child(
            ui::text(div(), TextSize::Sm)
              .font_weight(FontWeight::SEMIBOLD)
              .child("Feature Preview"),
          )
          .child(help(
            "Turn on experimental features. Off hides them in the UI and blocks the matching backend commands. Dev builds start with every preview flag on. Release builds use the shipped default until you change a switch.",
            p,
            4.,
          )),
      )
      .child(
        div()
          .flex()
          .flex_col()
          .gap_4()
          .children(preview.features.iter().map(|feature| {
            div()
              .flex()
              .items_center()
              .justify_between()
              .gap_4()
              .rounded(radius::MD)
              .border_1()
              .border_color(p.border)
              .p_4()
              .child(
                ui::text(div(), TextSize::Sm)
                  .min_w_0()
                  .font_weight(FontWeight::MEDIUM)
                  .child(feature.title.clone()),
              )
              .child(
                div()
                  .flex()
                  .items_center()
                  .gap_2()
                  .flex_shrink_0()
                  .child(
                    Button::new(format!("settings-preview-docs-{}", feature.id), p)
                      .variant(Variant::Ghost)
                      .size(Size::Sm)
                      .label("Docs")
                      .icon(theme::ICON_EXTERNAL_LINK, 12.)
                      .aria_label(format!("{} docs", feature.title))
                      .on_click(self.press(&format!("preview.docs:{}", feature.id))),
                  )
                  .child(
                    form::switch(
                      format!("settings-preview-{}", feature.id),
                      feature.title.clone(),
                      feature.checked,
                      false,
                      p,
                      self.field(&format!("preview.{}", feature.id)),
                    ),
                  ),
              )
          })),
      )
  }

  fn skills(
    &mut self,
    tab: &SkillsTab,
    p: &Palette,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> impl IntoElement {
    let query = self.controls.input(
      "skills.query",
      &tab.query,
      "Search by name or description",
      window,
      cx,
    );
    let scope = self
      .controls
      .select("skills.scope", &tab.scope_options, &tab.scope, window, cx);
    let level_options = options(&[("application", "Application"), ("repository", "Repository")]);
    let mut rows = Vec::new();
    for row in &tab.rows {
      let level = row.installed.as_ref().map(|value| {
        self.controls.select(
          &format!("skills.level:{}", row.id),
          &level_options,
          value,
          window,
          cx,
        )
      });
      rows.push(self.skill_row(row, level, p));
    }
    div()
      .flex()
      .flex_col()
      .gap_4()
      .debug_selector(|| "skill-library-settings".into())
      .child(
        div()
          .child(
            ui::text(div(), TextSize::Sm)
              .font_weight(FontWeight::SEMIBOLD)
              .child("Skill library"),
          )
          .child(help(
            "Browse the Treq skill registry and install skills into this application or the current repository. Installed skills are copied into new workspaces after checksum verification.",
            p,
            4.,
          ))
          .child(help(format!("{} installed", tab.installed), p, 4.)),
      )
      .child(
        div()
          .flex()
          .flex_wrap()
          .items_end()
          .gap_3()
          .child(
            div()
              .min_w(zpx(192.))
              .flex_1()
              .child(label("Search skills"))
              .child(div().mt_2().child(form::input_field(
                "skills-search",
                &query,
                false,
                false,
              ))),
          )
          .child(
            div()
              .min_w(zpx(160.))
              .child(label("Install level"))
              .child(div().mt_2().child(form::select_field("skills-scope", "Install level", &scope, false))),
          ),
      )
      .when(tab.loading, |this| {
        this.child(ui::delayed(
          "skills-loading",
          ui::text(div(), TextSize::Sm)
            .flex()
            .items_center()
            .gap_2()
            .text_color(p.muted_foreground)
            .child(ui::spinner("skills-loading", 16., p.muted_foreground))
            .child("Loading skill registry…"),
        ))
      })
      .when_some(tab.error.clone(), |this, err| {
        this.child(
          ui::text(div(), TextSize::Sm)
            .text_color(p.destructive)
            .child(err),
        )
      })
      .when(!tab.loading && tab.error.is_none() && tab.rows.is_empty(), |this| {
        this.child(
          ui::text(div(), TextSize::Sm)
            .text_color(p.muted_foreground)
            .child("No skills match this search."),
        )
      })
      .child(div().flex().flex_col().gap_3().children(rows))
  }

  /// `SkillRow`: one registry skill, with its install control.
  fn skill_row(
    &self,
    row: &SkillRow,
    level: Option<gpui::Entity<form::SelectStateOf>>,
    p: &Palette,
  ) -> gpui::Div {
    let id = &row.id;
    let selector = format!("skill-row-{id}");
    div()
      .relative()
      .flex()
      .flex_col()
      .gap_2()
      .border_1()
      .border_color(p.border)
      .rounded(radius::MD)
      .p_3()
      .pr_12()
      .debug_selector(move || selector)
      .when(row.installed.is_some(), |this| {
        this.child(
          div().absolute().top_2().right_2().child(
            Button::new(format!("skill-uninstall-{id}"), p)
              .aria_label("Uninstall")
              .variant(Variant::Outline)
              .size(Size::IconXs)
              .disabled(row.busy)
              .border_color(p.destructive.opacity(0.3))
              .text_color(p.destructive)
              .hover_bg(p.destructive.opacity(0.1))
              .icon(theme::ICON_TRASH, 14.)
              .on_click(self.press(&format!("skills.uninstall:{id}"))),
          ),
        )
      })
      .child(
        div()
          // React's `space-y-2` counts the absolute uninstall button as the
          // first child, so an installed skill's text starts 8px lower.
          .when(row.installed.is_some(), |this| this.mt_2())
          .flex()
          .items_start()
          .justify_between()
          .gap_3()
          .child(
            div()
              .min_w_0()
              .child(
                div()
                  .flex()
                  .items_center()
                  .gap_2()
                  .child(
                    ui::text(div(), TextSize::Sm)
                      .font_weight(FontWeight::MEDIUM)
                      .truncate()
                      .child(row.name.clone()),
                  )
                  .child(
                    ui::text(div(), TextSize::Xs)
                      .text_color(p.muted_foreground)
                      .child(row.source.clone()),
                  ),
              )
              .when_some(row.description.clone(), |this, description| {
                this.child(
                  ui::text(div().mt_1(), TextSize::Sm)
                    .text_color(p.muted_foreground)
                    .line_clamp(2)
                    .text_ellipsis()
                    .child(description),
                )
              }),
          )
          .when(row.has_url, |this| {
            let source = SharedString::from(format!("skill-source-{id}"));
            let selector = source.to_string();
            this.child(
              div()
                .id(source)
                .debug_selector(move || selector)
                .flex_none()
                .mr_6()
                .cursor_pointer()
                .child(ui::icon(theme::ICON_BOOK_OPEN, 16., p.muted_foreground))
                .on_click(self.press(&format!("skills.source:{id}"))),
            )
          }),
      )
      .map(|this| {
        if row.proprietary {
          this.child(
            ui::text(div(), TextSize::Sm)
              .text_color(p.muted_foreground)
              .child("Proprietary skills cannot be installed from the registry."),
          )
        } else if let Some(level) = level {
          this.child(
            div()
              .flex()
              .items_center()
              .justify_end()
              .gap_2()
              .child(
                ui::text(div(), TextSize::Sm)
                  .font_weight(FontWeight::MEDIUM)
                  .child("Install level"),
              )
              .child(div().w(zpx(128.)).child(form::select_field(
                format!("skill-level-{id}"),
                "Install level",
                &level,
                row.busy,
              ))),
          )
        } else {
          this.child(
            div().flex().justify_end().child(
              Button::new(format!("skill-install-{id}"), p)
                .size(Size::Sm)
                .disabled(row.busy)
                .label("Install…")
                .on_click(self.press(&format!("skills.install:{id}"))),
            ),
          )
        }
      })
  }

  /// The install dialog: where to store the skill.
  fn skill_dialog(&self, dialog: &SkillDialog, p: &Palette) -> impl IntoElement {
    // "**Repository** writes …": the name bold within the sentence.
    let paragraph = |strong: &str, rest: &str| {
      let text = format!("{strong} {rest}");
      let bold = gpui::HighlightStyle {
        font_weight: Some(FontWeight::MEDIUM),
        ..Default::default()
      };
      div().child(gpui::StyledText::new(text).with_highlights([(0..strong.len(), bold)]))
    };
    div()
      .id("skill-install-dialog-backdrop")
      .absolute()
      .inset_0()
      .flex()
      .items_center()
      .justify_center()
      .bg(p.scrim())
      .occlude()
      .child(
        div()
          .id("skill-install-dialog")
          .debug_selector(|| "skill-install-dialog".into())
          .w_full()
          .max_w(zpx(576.))
          .mx_4()
          .flex()
          .flex_col()
          .gap_4()
          .p_6()
          .rounded(radius::MD)
          .border_1()
          .border_color(p.border)
          .bg(p.background)
          .shadow_lg()
          .child(
            div()
              .flex()
              .flex_col()
              .gap_2()
              .child(
                ui::text(div(), TextSize::Lg)
                  .font_weight(FontWeight::SEMIBOLD)
                  .child(format!("Install {}", dialog.name)),
              )
              .child(
                ui::text(div(), TextSize::Sm)
                  .text_color(p.muted_foreground)
                  .child("Choose whether this skill is stored for every repository on this machine, or only for the current repository."),
              ),
          )
          .child(
            ui::text(div(), TextSize::Sm)
              .py_2()
              .flex()
              .flex_col()
              .gap_3()
              .child(paragraph(
                "Repository",
                "writes the skill under .treq/skills/ in this repository. Only this repository uses it.",
              ))
              .child(paragraph(
                "Application",
                "writes the skill into Treq application data. Every repository on this machine can use it.",
              )),
          )
          .child(
            div()
              .flex()
              .justify_end()
              .gap_2()
              .child(
                Button::new("skill-dialog-cancel", p)
                  .variant(Variant::Outline)
                  .disabled(dialog.busy)
                  .label("Cancel")
                  .on_click(self.press("skills.dialog.cancel")),
              )
              .child(
                Button::new("skill-dialog-application", p)
                  .variant(Variant::Outline)
                  .disabled(dialog.busy)
                  .label("Install for application")
                  .on_click(self.press("skills.dialog:application")),
              )
              .child(
                Button::new("skill-dialog-repository", p)
                  .disabled(dialog.busy)
                  .label("Install for repository")
                  .on_click(self.press("skills.dialog:repository")),
              ),
          ),
      )
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn previews_the_branch_an_edited_pattern_gives() {
    assert_eq!(
      branch_preview("treq/{name}"),
      "treq/{name} → treq/add-dark-mode"
    );
    assert_eq!(branch_preview("qa/{name}"), "qa/{name} → qa/add-dark-mode");
    assert_eq!(branch_preview("  "), "treq/{name} → treq/add-dark-mode");
  }
}
