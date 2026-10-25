//! What the settings screen reads from and writes to the backend, with the
//! defaults React's `SettingsPage` applies to what it reads.

use std::collections::HashMap;

use super::{account_data::AccountHost, checklist::ToolsHost, settings::RepositoryFields};
use crate::{
  core::{
    app_setup_script::{self, AppSetupScriptStatus},
    feature_preview::{self, PreviewFeature},
    skills::{
      self, CustomSkill, CustomizedSkill, InstalledSkill, SkillCatalogEntry, SkillCatalogSkillView,
      SkillInstallScope,
    },
  },
  gpui_shell::native::{Backend, NativeEvent},
  repo_config::{self, RepoConfig},
};

pub type Values = HashMap<String, Option<String>>;

/// What the settings screen asks of the backend; faked in tests. Everything
/// but `notify` and `open_url` blocks, so it is called off the main thread.
pub trait SettingsHost: ToolsHost + AccountHost {
  fn settings(&self, keys: &[String]) -> Result<Values, String>;
  fn set_settings(&self, values: &[(String, String)]) -> Result<(), String>;
  fn setup_script(&self) -> Result<AppSetupScriptStatus, String>;
  fn save_setup_script(&self, script: &str, always_run: bool) -> Result<(), String>;
  /// Starts a run; the status reports it as running once this returns.
  fn run_setup_script(&self);
  /// Parses `.treq/config.yaml` and writes what it sets into the repository's
  /// settings, as `load_repo_yaml_config`.
  fn repo_config(&self, repo_path: &str) -> Result<RepoConfig, String>;
  fn repo_settings(&self, repo_path: &str, keys: &[String]) -> Result<Values, String>;
  /// As `set_repo_setting`, for each value.
  fn set_repo_settings(&self, repo_path: &str, values: &[(String, String)]) -> Result<(), String>;
  /// The skill registry, each skill with its install state (`list_skill_catalog`).
  fn skill_catalog(&self, repo_path: Option<&str>) -> Result<Vec<SkillCatalogSkillView>, String>;
  /// The registry as of its last read, if there was one, without the
  /// network: shown while `skill_catalog` reads it again.
  fn skill_catalog_cached(&self, _repo_path: Option<&str>) -> Option<Vec<SkillCatalogSkillView>> {
    None
  }
  fn installed_skills(&self, repo_path: Option<&str>) -> Result<Vec<InstalledSkill>, String>;
  fn install_skill(
    &self,
    skill_id: &str,
    scope: SkillInstallScope,
    repo_path: Option<&str>,
  ) -> Result<(), String>;
  fn uninstall_skill(&self, skill_id: &str, repo_path: Option<&str>) -> Result<(), String>;
  fn set_skill_scope(
    &self,
    skill_id: &str,
    scope: SkillInstallScope,
    repo_path: Option<&str>,
  ) -> Result<(), String>;
  /// User-owned skills in the repository and the home folder.
  fn custom_skills(&self, repo_path: Option<&str>) -> Vec<CustomSkill>;
  /// Writes a user-owned copy of an installed skill into the repository.
  fn customize_skill(&self, skill_id: &str, repo_path: &str) -> Result<CustomizedSkill, String>;
  /// Downloads a registry skill not installed yet and writes it into the
  /// repository as a user-owned copy, checked against its checksum.
  fn customize_registry_skill(
    &self,
    skill_id: &str,
    repo_path: &str,
  ) -> Result<CustomizedSkill, String>;
  /// Copies a skill from the home folder (`source`) into the repository.
  fn customize_global_skill(
    &self,
    name: &str,
    source: &str,
    repo_path: &str,
  ) -> Result<CustomizedSkill, String>;
  /// The bytes at `url`, for the account's avatar.
  fn fetch_image(&self, url: &str) -> Result<Vec<u8>, String>;
  fn notify(&self, event: NativeEvent);
}

impl SettingsHost for Backend {
  fn settings(&self, keys: &[String]) -> Result<Values, String> {
    self.with_db(|db| db.get_settings_batch(keys).map_err(|e| e.to_string()))
  }

  fn set_settings(&self, values: &[(String, String)]) -> Result<(), String> {
    self.with_db(|db| {
      values
        .iter()
        .try_for_each(|(key, value)| db.set_setting(key, value))
        .map_err(|e| e.to_string())
    })
  }

  fn setup_script(&self) -> Result<AppSetupScriptStatus, String> {
    self.with_db(app_setup_script::get_status)
  }

  fn save_setup_script(&self, script: &str, always_run: bool) -> Result<(), String> {
    self.with_db(|db| app_setup_script::save(db, script, always_run))
  }

  fn run_setup_script(&self) {
    app_setup_script::spawn_run_if_needed(app_setup_script::Trigger::Manual);
  }

  fn repo_config(&self, repo_path: &str) -> Result<RepoConfig, String> {
    let config = repo_config::parse_config(repo_path)?;
    self.with_db(|db| repo_config::sync_to_settings(db, repo_path, &config))?;
    Ok(config)
  }

  fn repo_settings(&self, repo_path: &str, keys: &[String]) -> Result<Values, String> {
    // A remote repository has no settings yet: every key reads its default.
    if self.remote_containing(repo_path).is_some() {
      return Ok(keys.iter().map(|key| (key.clone(), None)).collect());
    }
    self.with_db(|db| {
      keys
        .iter()
        .map(|key| Ok((key.clone(), db.get_repo_setting(repo_path, key)?)))
        .collect::<rusqlite::Result<Values>>()
        .map_err(|e| e.to_string())
    })
  }

  fn set_repo_settings(&self, repo_path: &str, values: &[(String, String)]) -> Result<(), String> {
    if self.remote_containing(repo_path).is_some() {
      return Err(crate::gpui_shell::remote_repo::unsupported(
        "Changing repository settings",
      ));
    }
    self.with_db(|db| save_repo_settings(db, repo_path, values))
  }

  fn skill_catalog(&self, repo_path: Option<&str>) -> Result<Vec<SkillCatalogSkillView>, String> {
    let view = crate::gpui_shell::block_on(crate::commands::skills::catalog_view(repo_path, None))?;
    Ok(view.skills)
  }

  fn skill_catalog_cached(&self, repo_path: Option<&str>) -> Option<Vec<SkillCatalogSkillView>> {
    crate::commands::skills::cached_catalog_view(repo_path, None).map(|view| view.skills)
  }

  fn installed_skills(&self, repo_path: Option<&str>) -> Result<Vec<InstalledSkill>, String> {
    skills::list_installed_skills(repo_path)
  }

  fn install_skill(
    &self,
    skill_id: &str,
    scope: SkillInstallScope,
    repo_path: Option<&str>,
  ) -> Result<(), String> {
    crate::gpui_shell::block_on(crate::commands::skills::download_and_install(
      skill_id, scope, repo_path, None,
    ))?;
    self.with_db(skills::persist_app_index_setting)
  }

  fn uninstall_skill(&self, skill_id: &str, repo_path: Option<&str>) -> Result<(), String> {
    skills::uninstall_skill(skill_id, repo_path)?;
    self.with_db(skills::persist_app_index_setting)
  }

  fn set_skill_scope(
    &self,
    skill_id: &str,
    scope: SkillInstallScope,
    repo_path: Option<&str>,
  ) -> Result<(), String> {
    skills::set_skill_install_scope(skill_id, scope, repo_path)?;
    self.with_db(skills::persist_app_index_setting)
  }

  fn custom_skills(&self, repo_path: Option<&str>) -> Vec<CustomSkill> {
    let repo = repo_path.filter(|repo| self.remote_containing(repo).is_none());
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    skills::list_custom_skills(repo, home.as_deref())
  }

  fn customize_skill(&self, skill_id: &str, repo_path: &str) -> Result<CustomizedSkill, String> {
    if self.remote_containing(repo_path).is_some() {
      return Err(crate::gpui_shell::remote_repo::unsupported(
        "Customizing a skill",
      ));
    }
    skills::customize_skill(skill_id, repo_path)
  }

  fn customize_registry_skill(
    &self,
    skill_id: &str,
    repo_path: &str,
  ) -> Result<CustomizedSkill, String> {
    if self.remote_containing(repo_path).is_some() {
      return Err(crate::gpui_shell::remote_repo::unsupported(
        "Customizing a skill",
      ));
    }
    let (entry, files) =
      crate::gpui_shell::block_on(crate::commands::skills::download_verified(skill_id, None))?;
    skills::customize_skill_from_files(&entry.id, &entry.name, &files, repo_path)
  }

  fn customize_global_skill(
    &self,
    name: &str,
    source: &str,
    repo_path: &str,
  ) -> Result<CustomizedSkill, String> {
    if self.remote_containing(repo_path).is_some() {
      return Err(crate::gpui_shell::remote_repo::unsupported(
        "Customizing a skill",
      ));
    }
    skills::customize_global_skill(name, std::path::Path::new(source), repo_path)
  }

  fn fetch_image(&self, url: &str) -> Result<Vec<u8>, String> {
    crate::gpui_shell::block_on(async {
      let response = reqwest::get(url)
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|e| e.to_string())?;
      let bytes = response.bytes().await.map_err(|e| e.to_string())?;
      Ok(bytes.to_vec())
    })
  }

  fn notify(&self, event: NativeEvent) {
    Backend::notify(self, event);
  }
}

/// `skillMatches`: the skills the search leaves.
pub fn filter_skills<'a>(
  skills: &'a [SkillCatalogSkillView],
  query: &str,
) -> Vec<&'a SkillCatalogSkillView> {
  let query = query.trim().to_lowercase();
  skills
    .iter()
    .filter(|view| {
      let skill = &view.skill;
      let haystack = format!(
        "{} {} {} {}",
        skill.name,
        skill.description.as_deref().unwrap_or_default(),
        skill.source,
        skill.category.as_deref().unwrap_or_default()
      )
      .to_lowercase();
      query.is_empty() || haystack.contains(&query)
    })
    .collect()
}

pub fn scope_value(scope: SkillInstallScope) -> &'static str {
  match scope {
    SkillInstallScope::Application => "application",
    SkillInstallScope::Repository => "repository",
  }
}

pub fn parse_scope(value: &str) -> Option<SkillInstallScope> {
  match value {
    "application" => Some(SkillInstallScope::Application),
    "repository" => Some(SkillInstallScope::Repository),
    _ => None,
  }
}

// Matches `DEFAULT_SKILLS_CATALOG_URL`: the catalog and its pages live there.
const SKILL_REGISTRY_ORIGIN: &str = "https://treq.dev";

/// The skill's page on the treq.dev registry, or the registry itself.
pub fn skill_registry_url(route: Option<&str>) -> String {
  match route {
    Some(route) if route.starts_with("/skills/") => format!("{SKILL_REGISTRY_ORIGIN}{route}"),
    _ => format!("{SKILL_REGISTRY_ORIGIN}/skills"),
  }
}

fn format_bytes(bytes: u64) -> String {
  const KB: f64 = 1024.;
  let b = bytes as f64;
  if b < KB {
    format!("{bytes} B")
  } else if b < KB * KB {
    format!("{:.1} KB", b / KB)
  } else {
    format!("{:.1} MB", b / (KB * KB))
  }
}

/// The catalog metadata a link's install confirmation lists.
pub fn skill_details(skill: &SkillCatalogEntry) -> Vec<(String, String)> {
  let count = skill.files.len();
  let size: u64 = skill.files.iter().map(|f| f.size).sum();
  let files = format!(
    "{count} {}, {}",
    if count == 1 { "file" } else { "files" },
    format_bytes(size)
  );
  [
    ("Catalog id", Some(skill.id.clone())),
    ("Source", Some(skill.source.clone())),
    ("Category", skill.category.clone()),
    ("License", skill.license.clone()),
    ("Files", Some(files)),
  ]
  .into_iter()
  .filter_map(|(label, value)| Some((label.to_string(), value.filter(|v| !v.is_empty())?)))
  .collect()
}

pub const NOTIFY_AGENT_FINISHED: &str = "notify_agent_finished";

/// Each preview feature's title and its page on the website.
pub const PREVIEW_FEATURES: [(PreviewFeature, &str, &str); 5] = [
  (
    PreviewFeature::SkillsInstallation,
    "Skills installation",
    "/docs/how-to/installing-skills",
  ),
  (
    PreviewFeature::RemoteSsh,
    "Remote SSH",
    "/docs/how-to/remote-ssh-workspaces",
  ),
  (
    PreviewFeature::LinearIntegration,
    "Linear integration",
    "/docs/concepts/linear-integration",
  ),
  (PreviewFeature::Logs, "Logs", "/docs/concepts/logs"),
  (
    PreviewFeature::Devcontainers,
    "Devcontainer environments",
    "/docs/how-to/devcontainer-environments",
  ),
];

/// `MIN_ZOOM`, `MAX_ZOOM` and `ZOOM_STEP`.
pub const ZOOM: (f32, f32, f32) = (50., 200., 5.);

/// `FONT_SIZE_PRESETS`: size and label.
pub const FONT_SIZES: [(u32, &str); 5] = [
  (10, "Extra Small"),
  (12, "Small"),
  (14, "Medium (default)"),
  (16, "Large"),
  (18, "Extra Large"),
];

/// `nearestFontSizePreset`: older builds allowed any size from 8 to 32.
pub fn nearest_font_size(size: u32) -> u32 {
  FONT_SIZES
    .iter()
    .map(|(preset, _)| *preset)
    .fold(FONT_SIZES[0].0, |best, preset| {
      if preset.abs_diff(size) < best.abs_diff(size) {
        preset
      } else {
        best
      }
    })
}

/// The Application tab's values, as last loaded or edited.
#[derive(Debug, Clone, PartialEq)]
pub struct AppFields {
  pub theme: String,
  pub zoom: f32,
  /// The stored size, saved as is unless a preset is picked.
  pub font_size: u32,
  pub default_model: String,
  pub default_agent: String,
  pub conflict_marker_style: String,
  pub notify_agent_finished: bool,
}

/// The keys the screen loads: the Application tab's and the preview flags.
pub fn app_keys() -> Vec<String> {
  [
    "theme",
    "ui_zoom",
    "terminal_font_size",
    "default_model",
    "default_agent",
    "conflict_marker_style",
    NOTIFY_AGENT_FINISHED,
  ]
  .into_iter()
  .map(str::to_string)
  .chain(PREVIEW_FEATURES.iter().map(|(f, _, _)| f.setting_key()))
  .collect()
}

fn int_in(values: &Values, key: &str, min: u32, max: u32) -> Option<u32> {
  let value = values.get(key)?.as_deref()?.trim().parse::<u32>().ok()?;
  (min..=max).contains(&value).then_some(value)
}

fn string(values: &Values, key: &str) -> Option<String> {
  values.get(key).cloned().flatten()
}

impl AppFields {
  /// With React's defaults (`settingsHydration`, `SettingsPage`).
  pub fn from_values(values: &Values) -> Self {
    let theme = string(values, "theme")
      .filter(|theme| ["light", "dark", "system"].contains(&theme.as_str()))
      .unwrap_or_else(|| "system".into());
    Self {
      theme,
      zoom: int_in(values, "ui_zoom", ZOOM.0 as u32, ZOOM.1 as u32).unwrap_or(100) as f32,
      font_size: int_in(values, "terminal_font_size", 8, 32).unwrap_or(14),
      default_model: string(values, "default_model").unwrap_or_default(),
      default_agent: string(values, "default_agent").unwrap_or_default(),
      conflict_marker_style: string(values, "conflict_marker_style")
        .unwrap_or_else(|| "git".into()),
      // On unless turned off, as the backend reads it.
      notify_agent_finished: string(values, NOTIFY_AGENT_FINISHED)
        .is_none_or(|value| value.trim() != "false"),
    }
  }

  /// What Save writes, in `handleSaveApplicationSettings`'s order.
  pub fn to_values(&self) -> Vec<(String, String)> {
    [
      ("default_model", self.default_model.clone()),
      ("default_agent", self.default_agent.clone()),
      ("conflict_marker_style", self.conflict_marker_style.clone()),
      (
        NOTIFY_AGENT_FINISHED,
        self.notify_agent_finished.to_string(),
      ),
      ("terminal_font_size", self.font_size.to_string()),
      ("ui_zoom", (self.zoom.round() as i64).to_string()),
    ]
    .map(|(key, value)| (key.to_string(), value))
    .to_vec()
  }
}

/// Whether each preview is on, as `parsePreviewSetting`.
pub fn preview_flags(values: &Values) -> Vec<(PreviewFeature, bool)> {
  PREVIEW_FEATURES
    .iter()
    .map(|(feature, _, _)| {
      let on = match string(values, &feature.setting_key()).as_deref() {
        Some("true") => true,
        Some("false") => false,
        _ => feature_preview::compile_default(*feature),
      };
      (*feature, on)
    })
    .collect()
}

/// The repository settings behind each field, in the order React saves them.
const REPO_KEYS: [&str; 8] = [
  "branch_name_pattern",
  "included_copy_files",
  "default_model",
  "default_agent",
  "auto_push",
  "review_prompt",
  "review_agent",
  "auto_review_trigger",
];

pub fn repo_keys() -> Vec<String> {
  REPO_KEYS.map(str::to_string).to_vec()
}

/// `normalizeAutoReviewTrigger`: `"on-push"` is the old name of `"on-pull"`.
fn auto_review_trigger(stored: Option<&str>) -> String {
  match stored {
    Some("on-push") => "on-pull",
    Some(trigger @ ("on-commit" | "on-rebase" | "on-pull")) => trigger,
    _ => "off",
  }
  .to_string()
}

impl Default for RepositoryFields {
  fn default() -> Self {
    Self::from_values(&Values::new())
  }
}

impl RepositoryFields {
  pub fn from_values(values: &Values) -> Self {
    let get = |key: &str| string(values, key).filter(|value| !value.is_empty());
    Self {
      branch_name_pattern: get("branch_name_pattern").unwrap_or_else(|| "treq/{name}".into()),
      included_files: get("included_copy_files").unwrap_or_default(),
      default_model: get("default_model").unwrap_or_default(),
      default_agent: get("default_agent").unwrap_or_default(),
      auto_push: get("auto_push").as_deref() == Some("true"),
      review_prompt: get("review_prompt").unwrap_or_default(),
      review_agent: get("review_agent").unwrap_or_default(),
      auto_review_trigger: auto_review_trigger(get("auto_review_trigger").as_deref()),
    }
  }

  pub fn to_values(&self) -> Vec<(String, String)> {
    let values = [
      self.branch_name_pattern.clone(),
      self.included_files.clone(),
      self.default_model.clone(),
      self.default_agent.clone(),
      self.auto_push.to_string(),
      self.review_prompt.clone(),
      self.review_agent.clone(),
      self.auto_review_trigger.clone(),
    ];
    REPO_KEYS
      .iter()
      .zip(values)
      .map(|(key, value)| (key.to_string(), value))
      .collect()
  }

  /// Sets the field GPUI names `field` (as in `"autoPush"`) to `value`.
  pub fn set(&mut self, field: &str, value: &serde_json::Value) {
    let text = || value.as_str().unwrap_or_default().to_string();
    let flag = || value.as_bool().unwrap_or_default();
    match field {
      "branchNamePattern" => self.branch_name_pattern = text(),
      "includedFiles" => self.included_files = text(),
      "defaultModel" => self.default_model = text(),
      "defaultAgent" => self.default_agent = text(),
      "autoPush" => self.auto_push = flag(),
      "reviewPrompt" => self.review_prompt = text(),
      "reviewAgent" => self.review_agent = text(),
      "autoReviewTrigger" => self.auto_review_trigger = text(),
      _ => {}
    }
  }
}

/// The fields `.treq/config.yaml` sets, which the screen disables.
pub fn managed_fields(config: &RepoConfig) -> Vec<String> {
  [
    ("branchNamePattern", config.branch_name_pattern.is_some()),
    ("includedFiles", config.included_copy_files.is_some()),
    ("defaultModel", config.default_model.is_some()),
    ("defaultAgent", config.default_agent.is_some()),
    ("reviewPrompt", config.review_prompt.is_some()),
    ("reviewAgent", config.review_agent.is_some()),
    ("autoReviewTrigger", config.auto_review_trigger.is_some()),
  ]
  .into_iter()
  .filter(|(_, managed)| *managed)
  .map(|(field, _)| field.to_string())
  .collect()
}

/// `RepoYamlConfigCard`'s `hasAnyValue`: whether the card shows.
pub fn sets_any_value(config: &RepoConfig) -> bool {
  config.branch_name_pattern.is_some()
    || config.default_model.is_some()
    || config.default_agent.is_some()
    || config.target_branch.is_some()
    || config
      .included_copy_files
      .as_ref()
      .is_some_and(|files| !files.is_empty())
}

/// `formatStatus` for the Run row.
pub fn setup_status_text(status: &AppSetupScriptStatus) -> String {
  if status.running {
    return "Running…".into();
  }
  let Some(ran_at) = &status.last_run_at else {
    return "Never run".into();
  };
  // `toLocaleString()` in an en-US locale.
  let ran_at = chrono::DateTime::parse_from_rfc3339(ran_at)
    .map(|at| {
      at.with_timezone(&chrono::Local)
        .format("%-m/%-d/%Y, %-I:%M:%S %p")
        .to_string()
    })
    .unwrap_or_else(|_| ran_at.clone());
  format!(
    "Last run {ran_at} · {}",
    status.last_status.as_deref().unwrap_or("unknown")
  )
}

/// `set_repo_setting` for each value.
pub fn save_repo_settings(
  db: &crate::db::Database,
  repo_path: &str,
  values: &[(String, String)],
) -> Result<(), String> {
  for (key, value) in values {
    db.set_repo_setting(repo_path, key, value)
      .map_err(|e| e.to_string())?;
  }
  Ok(())
}
